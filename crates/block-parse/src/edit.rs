//! Tree operations, addressed by [`BlockId`] because positions go stale.

use crate::language::{BlockKind, Fit, Language};
use crate::program::{Block, BlockId, MAX_DEPTH, Program, Slot, Stack, find_in};

/// Blocks out of the program: a statement and everything below it, or one
/// reporter.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    /// Head first. Never empty.
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Location {
    Stack { stack: usize, index: usize },
    Branch { parent: BlockId, branch: String, index: usize },
    Input { parent: BlockId, slot: Slot },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Free { pos: [f32; 2] },
    /// Splices in if something follows.
    After(BlockId),
    BranchStart { parent: BlockId, branch: String },
    /// The stack moves to `pos` so its blocks stay put on screen. The caller
    /// supplies it because only layout knows the fragment's height.
    Above { head: BlockId, pos: [f32; 2] },
    /// Ejects any reporter already there. A list's empty slot appends.
    Input { parent: BlockId, slot: Slot },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttachError {
    NoSuchBlock(BlockId),
    NoSuchBranch { parent: BlockId, branch: String },
    /// Also an index past a list's empty slot.
    NoSuchInput { parent: BlockId, slot: Slot },
    HatNotAtTop,
    AfterCap,
    /// A fragment ending in a cap would cut off the blocks below the target.
    CapWouldOrphan,
    /// A reporter into a stack, or a statement into a slot.
    WrongKind,
    TypeMismatch { slot: String, output: String },
    /// Unknown opcodes may only be dropped free.
    UnknownOpcode(String),
    /// `depth` is what the fragment's deepest block would reach, past
    /// `MAX_DEPTH`.
    TooDeep { depth: usize },
}

impl Fragment {
    /// Nesting levels the fragment spans.
    pub fn height(&self) -> usize {
        self.blocks.iter().map(Block::height).max().unwrap_or(0)
    }
}

impl Program {
    /// An emptied top-level stack is removed.
    pub fn detach(&mut self, id: BlockId) -> Option<Fragment> {
        let blocks = self.take_run(id)?;
        self.reserve_ids(&blocks);
        Some(Fragment { blocks })
    }

    fn take_run(&mut self, id: BlockId) -> Option<Vec<Block>> {
        for index in 0..self.stacks.len() {
            let stack = &mut self.stacks[index].blocks;
            if let Some(position) = stack.iter().position(|block| block.id == id) {
                let taken = stack.split_off(position);
                if stack.is_empty() {
                    self.stacks.remove(index);
                }
                return Some(taken);
            }
        }
        self.stacks
            .iter_mut()
            .flat_map(|stack| stack.blocks.iter_mut())
            .find_map(|block| take_run_in(block, id))
    }

    pub fn can_attach(
        &self,
        language: &Language,
        fragment: &Fragment,
        target: &Target,
    ) -> Result<(), AttachError> {
        self.check_attach(language, fragment, target, None)
    }

    /// [`can_attach`](Self::can_attach) as if the run `fragment` copies were
    /// already detached, so a drag need not copy the program. A head not in
    /// the program is judged as by `can_attach`.
    pub fn can_move(
        &self,
        language: &Language,
        fragment: &Fragment,
        target: &Target,
    ) -> Result<(), AttachError> {
        let head = fragment.blocks.first().map(|head| head.id);
        self.check_attach(language, fragment, target, head)
    }

    fn check_attach(
        &self,
        language: &Language,
        fragment: &Fragment,
        target: &Target,
        lifted: Option<BlockId>,
    ) -> Result<(), AttachError> {
        let Some(head) = fragment.blocks.first() else {
            return Err(AttachError::WrongKind);
        };
        if matches!(target, Target::Free { .. }) {
            return Ok(());
        }
        let is_lifted = |block: &Block| Some(block.id) == lifted;
        let gone = |id: BlockId| lifted.is_some() && find_in(&fragment.blocks, id).is_some();
        let find = |id: BlockId| self.find(id).filter(|_| !gone(id));
        let def = language
            .block(&head.opcode)
            .ok_or_else(|| AttachError::UnknownOpcode(head.opcode.clone()))?;
        let kind_of = |block: &Block| language.block(&block.opcode).map(|def| &def.kind);
        let ends_in_cap = fragment
            .blocks
            .last()
            .is_some_and(|last| kind_of(last) == Some(&BlockKind::Cap));
        let statement = || match def.kind {
            BlockKind::Reporter(_) => Err(AttachError::WrongKind),
            BlockKind::Hat => Err(AttachError::HatNotAtTop),
            BlockKind::Statement | BlockKind::Cap => Ok(()),
        };

        let depth = match target {
            Target::Free { .. } => 1,
            Target::Input { parent, slot } => {
                let output = def.kind.output().ok_or(AttachError::WrongKind)?;
                if fragment.blocks.len() != 1 {
                    return Err(AttachError::WrongKind);
                }
                let owner = find(*parent).ok_or(AttachError::NoSuchBlock(*parent))?;
                let owner_def = language.block(&owner.opcode);
                let ty = match slot.item {
                    None => owner_def.and_then(|def| def.input(&slot.input)).map(|input| &input.ty),
                    Some(index) => owner_def
                        .and_then(|def| def.list(&slot.input))
                        .filter(|_| index <= owner.list_len(&slot.input, lifted))
                        .map(|list| &list.ty),
                };
                let ty = ty.ok_or_else(|| AttachError::NoSuchInput {
                    parent: *parent,
                    slot: slot.clone(),
                })?;
                if language.fit(output, ty) == Fit::No {
                    return Err(AttachError::TypeMismatch {
                        slot: ty.clone(),
                        output: output.to_owned(),
                    });
                }
                self.depth_of(*parent).unwrap_or(1) + 1
            }
            Target::After(id) => {
                statement()?;
                if gone(*id) {
                    return Err(AttachError::NoSuchBlock(*id));
                }
                let (seq, index) = self
                    .sequence_of(*id)
                    .ok_or(if self.find(*id).is_some() {
                        AttachError::WrongKind
                    } else {
                        AttachError::NoSuchBlock(*id)
                    })?;
                match kind_of(&seq[index]) {
                    Some(BlockKind::Cap) => return Err(AttachError::AfterCap),
                    Some(BlockKind::Reporter(_)) => return Err(AttachError::WrongKind),
                    _ => {}
                }
                let len = seq.iter().position(is_lifted).unwrap_or(seq.len());
                if ends_in_cap && index + 1 < len {
                    return Err(AttachError::CapWouldOrphan);
                }
                self.depth_of(*id).unwrap_or(1)
            }
            Target::BranchStart { parent, branch } => {
                statement()?;
                let owner = find(*parent).ok_or(AttachError::NoSuchBlock(*parent))?;
                let has_branch = language
                    .block(&owner.opcode)
                    .is_some_and(|def| def.has_branch(branch));
                if !has_branch {
                    return Err(AttachError::NoSuchBranch {
                        parent: *parent,
                        branch: branch.clone(),
                    });
                }
                let occupied = owner
                    .branches
                    .get(branch)
                    .and_then(|seq| seq.first())
                    .is_some_and(|first| !is_lifted(first));
                if ends_in_cap && occupied {
                    return Err(AttachError::CapWouldOrphan);
                }
                self.depth_of(*parent).unwrap_or(1) + 1
            }
            Target::Above { head: below, .. } => {
                if def.kind.output().is_some() {
                    return Err(AttachError::WrongKind);
                }
                let stack = self
                    .stacks
                    .iter()
                    .find(|stack| stack.blocks.first().is_some_and(|b| b.id == *below))
                    .filter(|_| !gone(*below))
                    .ok_or(AttachError::NoSuchBlock(*below))?;
                match kind_of(&stack.blocks[0]) {
                    Some(BlockKind::Hat) => return Err(AttachError::HatNotAtTop),
                    Some(BlockKind::Reporter(_)) => return Err(AttachError::WrongKind),
                    _ => {}
                }
                if ends_in_cap {
                    return Err(AttachError::CapWouldOrphan);
                }
                1
            }
        };

        let reached = depth + fragment.height() - 1;
        if reached > MAX_DEPTH {
            return Err(AttachError::TooDeep { depth: reached });
        }
        Ok(())
    }

    /// Returns any reporter pushed out of an occupied slot, for the caller to
    /// drop somewhere. On failure the fragment is handed back untouched.
    pub fn attach(
        &mut self,
        language: &Language,
        fragment: Fragment,
        target: Target,
    ) -> Result<Option<Fragment>, (AttachError, Fragment)> {
        if let Err(error) = self.can_attach(language, &fragment, &target) {
            return Err((error, fragment));
        }
        self.reserve_ids(&fragment.blocks);
        let blocks = fragment.blocks;
        match target {
            Target::Free { pos } => self.stacks.push(Stack { pos, blocks }),
            Target::After(id) => {
                let (seq, index) = self.sequence_of_mut(id).expect("checked by can_attach");
                seq.splice(index + 1..index + 1, blocks);
            }
            Target::BranchStart { parent, branch } => {
                let owner = self.find_mut(parent).expect("checked by can_attach");
                owner.branches.entry(branch).or_default().splice(0..0, blocks);
            }
            Target::Above { head, pos } => {
                let stack = self
                    .stacks
                    .iter_mut()
                    .find(|stack| stack.blocks.first().is_some_and(|b| b.id == head))
                    .expect("checked by can_attach");
                let below = std::mem::replace(&mut stack.blocks, blocks);
                stack.blocks.extend(below);
                stack.pos = pos;
            }
            Target::Input { parent, slot } => {
                let owner = self.find_mut(parent).expect("checked by can_attach");
                let reporter = blocks.into_iter().next().expect("checked by can_attach");
                let slot = owner.slot_entry(&slot).expect("checked by can_attach");
                let ejected = slot.block.replace(Box::new(reporter));
                return Ok(ejected.map(|block| Fragment {
                    blocks: vec![*block],
                }));
            }
        }
        Ok(None)
    }

    /// A copy of a block and, for a statement, everything below it, with
    /// fresh ids throughout. The program is unchanged.
    pub fn duplicate(&mut self, id: BlockId) -> Option<Fragment> {
        let mut fragment = self.run_at(id)?;
        for block in &mut fragment.blocks {
            self.renumber(block);
        }
        Some(fragment)
    }

    /// A copy of what [`detach`](Self::detach) would take at `id`, ids and all.
    pub fn run_at(&self, id: BlockId) -> Option<Fragment> {
        let blocks = match self.sequence_of(id) {
            Some((seq, index)) => seq[index..].to_vec(),
            None => vec![self.find(id)?.clone()],
        };
        Some(Fragment { blocks })
    }

    fn renumber(&mut self, block: &mut Block) {
        block.id = self.fresh_id();
        for input in block.inputs.values_mut().chain(block.lists.values_mut().flatten()) {
            if let Some(inner) = input.block.as_deref_mut() {
                self.renumber(inner);
            }
        }
        for seq in block.branches.values_mut() {
            for child in seq {
                self.renumber(child);
            }
        }
    }

    /// Deletes one block and what is nested in it; blocks below it close up.
    pub fn remove(&mut self, id: BlockId) -> Option<Block> {
        let removed = match self.locate(id)? {
            Location::Input { parent, slot } => {
                let owner = self.find_mut(parent)?;
                let removed = owner.slot_entry(&slot)?.block.take()?;
                owner.trim_lists();
                *removed
            }
            Location::Stack { .. } | Location::Branch { .. } => {
                let (seq, index) = self.sequence_of_mut(id)?;
                seq.remove(index)
            }
        };
        self.stacks.retain(|stack| !stack.blocks.is_empty());
        self.reserve_ids(std::slice::from_ref(&removed));
        Some(removed)
    }

    /// Where a run detached at `id` goes to be put back as it was. A whole
    /// stack returns to its position but last in stack order.
    pub fn home_of(&self, id: BlockId) -> Option<Target> {
        Some(match self.locate(id)? {
            Location::Stack { stack, index: 0 } => Target::Free {
                pos: self.stacks[stack].pos,
            },
            Location::Stack { stack, index } => Target::After(self.stacks[stack].blocks[index - 1].id),
            Location::Branch { parent, branch, index: 0 } => Target::BranchStart { parent, branch },
            Location::Branch { parent, branch, index } => {
                Target::After(self.find(parent)?.branches.get(&branch)?[index - 1].id)
            }
            Location::Input { parent, slot } => Target::Input { parent, slot },
        })
    }

    /// The stack or branch holding `id`, and its index there. `None` for a
    /// reporter in a slot.
    pub fn sequence_of(&self, id: BlockId) -> Option<(&[Block], usize)> {
        self.stacks
            .iter()
            .find_map(|stack| sequence_in(&stack.blocks, id))
    }

    fn sequence_of_mut(&mut self, id: BlockId) -> Option<(&mut Vec<Block>, usize)> {
        self.stacks
            .iter_mut()
            .find_map(|stack| sequence_in_mut(&mut stack.blocks, id))
    }
}

fn take_run_in(block: &mut Block, id: BlockId) -> Option<Vec<Block>> {
    let mut taken = None;
    for input in block.inputs.values_mut().chain(block.lists.values_mut().flatten()) {
        if input.block.as_ref().is_some_and(|inner| inner.id == id) {
            taken = input.block.take().map(|inner| vec![*inner]);
            break;
        }
    }
    if taken.is_some() {
        block.trim_lists();
        return taken;
    }
    for input in block.inputs.values_mut().chain(block.lists.values_mut().flatten()) {
        if let Some(inner) = input.block.as_deref_mut()
            && let Some(found) = take_run_in(inner, id)
        {
            return Some(found);
        }
    }
    for seq in block.branches.values_mut() {
        if let Some(position) = seq.iter().position(|child| child.id == id) {
            return Some(seq.split_off(position));
        }
        if let Some(found) = seq.iter_mut().find_map(|child| take_run_in(child, id)) {
            return Some(found);
        }
    }
    None
}

fn sequence_in(seq: &[Block], id: BlockId) -> Option<(&[Block], usize)> {
    if let Some(index) = seq.iter().position(|block| block.id == id) {
        return Some((seq, index));
    }
    seq.iter().find_map(|block| {
        block
            .reporters()
            .find_map(|inner| sequence_in(std::slice::from_ref(inner), id).filter(|_| inner.id != id))
            .or_else(|| block.branches.values().find_map(|seq| sequence_in(seq, id)))
    })
}

fn sequence_in_mut(seq: &mut Vec<Block>, id: BlockId) -> Option<(&mut Vec<Block>, usize)> {
    if let Some(index) = seq.iter().position(|block| block.id == id) {
        return Some((seq, index));
    }
    seq.iter_mut().find_map(|block| sequence_in_block_mut(block, id))
}

fn sequence_in_block_mut(block: &mut Block, id: BlockId) -> Option<(&mut Vec<Block>, usize)> {
    for input in block.inputs.values_mut().chain(block.lists.values_mut().flatten()) {
        if let Some(inner) = input.block.as_deref_mut()
            && let Some(found) = sequence_in_block_mut(inner, id)
        {
            return Some(found);
        }
    }
    block
        .branches
        .values_mut()
        .find_map(|seq| sequence_in_mut(seq, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::literal::Validators;
    use crate::program::Input;

    fn tiny() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap()
    }

    fn strict() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/strict_tiny.ron"),
            &Validators::new(),
        )
        .unwrap()
    }

    /// Drops fresh blocks of `opcodes` as one stack at the origin.
    fn stack(program: &mut Program, language: &Language, opcodes: &[&str]) -> Vec<BlockId> {
        let blocks: Vec<Block> = opcodes
            .iter()
            .map(|opcode| program.instantiate(language, opcode).unwrap())
            .collect();
        let ids = blocks.iter().map(|block| block.id).collect();
        program.stacks.push(Stack {
            pos: [0.0, 0.0],
            blocks,
        });
        ids
    }

    fn fresh(program: &mut Program, language: &Language, opcode: &str) -> Fragment {
        Fragment {
            blocks: vec![program.instantiate(language, opcode).unwrap()],
        }
    }

    fn opcodes(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|block| block.opcode.as_str()).collect()
    }

    #[test]
    fn detaching_takes_everything_below_and_drops_an_emptied_stack() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["when_run", "print", "set"]);

        let run = program.detach(ids[1]).unwrap();
        assert_eq!(opcodes(&run.blocks), ["print", "set"]);
        assert_eq!(opcodes(&program.stacks[0].blocks), ["when_run"]);

        program.detach(ids[0]).unwrap();
        assert!(program.stacks.is_empty());
    }

    #[test]
    fn after_splices_and_above_prepends() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["print", "set"]);

        let run = fresh(&mut program, &language, "while");
        program.attach(&language, run, Target::After(ids[0])).unwrap();
        assert_eq!(opcodes(&program.stacks[0].blocks), ["print", "while", "set"]);

        let hat = fresh(&mut program, &language, "when_run");
        let target = Target::Above {
            head: ids[0],
            pos: [0.0, -40.0],
        };
        program.attach(&language, hat, target).unwrap();
        assert_eq!(program.stacks[0].blocks[0].opcode, "when_run");
        assert_eq!(program.stacks[0].pos, [0.0, -40.0]);
    }

    #[test]
    fn hats_only_start_stacks_and_nothing_goes_above_them() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["when_run", "print"]);
        let hat = fresh(&mut program, &language, "when_run");

        assert_eq!(
            program.can_attach(&language, &hat, &Target::After(ids[1])),
            Err(AttachError::HatNotAtTop)
        );
        let print = fresh(&mut program, &language, "print");
        let above = Target::Above {
            head: ids[0],
            pos: [0.0, 0.0],
        };
        assert_eq!(
            program.can_attach(&language, &print, &above),
            Err(AttachError::HatNotAtTop)
        );
    }

    /// Tiny has no cap, and a cap is what makes where a run came from matter.
    fn with_cap() -> Language {
        Language::from_ron(
            r#"Language(
                name: "capped",
                file: (extension: "capped"),
                blocks: [
                    (id: "step", name: "Step", spec: "step"),
                    (id: "loop", name: "Loop", spec: "loop [body]"),
                    (id: "stop", name: "Stop", kind: Cap, spec: "stop"),
                ],
            )"#,
            &Validators::new(),
        )
        .unwrap()
    }

    #[test]
    fn a_run_moves_as_if_already_detached() {
        let language = with_cap();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["step", "loop", "stop"]);
        let inner = program.instantiate(&language, "stop").unwrap();
        let inner_id = inner.id;
        program.find_mut(ids[1]).unwrap().branches.get_mut("body").unwrap().push(inner);
        let body = Target::BranchStart {
            parent: ids[1],
            branch: "body".into(),
        };

        for (id, target) in [(ids[2], Target::After(ids[1])), (inner_id, body.clone())] {
            let run = program.clone().detach(id).unwrap();
            assert_eq!(
                program.can_attach(&language, &run, &target),
                Err(AttachError::CapWouldOrphan)
            );
            assert_eq!(program.can_move(&language, &run, &target), Ok(()));
        }

        // Blocks that stay put still count.
        let tail = program.clone().detach(ids[2]).unwrap();
        assert_eq!(
            program.can_move(&language, &tail, &Target::After(ids[0])),
            Err(AttachError::CapWouldOrphan)
        );
        assert_eq!(
            program.can_move(&language, &tail, &body),
            Err(AttachError::CapWouldOrphan)
        );

        let run = program.clone().detach(ids[1]).unwrap();
        assert_eq!(
            program.can_move(&language, &run, &body),
            Err(AttachError::NoSuchBlock(ids[1]))
        );
        let above = Target::Above {
            head: ids[0],
            pos: [0.0, 0.0],
        };
        let whole = program.clone().detach(ids[0]).unwrap();
        assert_eq!(
            program.can_move(&language, &whole, &above),
            Err(AttachError::NoSuchBlock(ids[0]))
        );

        let all = [ids[0], ids[1], ids[2], inner_id];
        let targets: Vec<Target> = all
            .iter()
            .map(|id| Target::After(*id))
            .chain([body, above])
            .collect();
        for id in all {
            let mut detached = program.clone();
            let run = detached.detach(id).unwrap();
            for target in &targets {
                assert_eq!(
                    program.can_move(&language, &run, target),
                    detached.can_attach(&language, &run, target),
                    "{id:?} to {target:?}"
                );
            }
        }
    }

    #[test]
    fn reporters_go_in_slots_and_eject_what_was_there() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["print"]);
        let slot = Target::Input {
            parent: ids[0],
            slot: Slot::input("value"),
        };

        let join = fresh(&mut program, &language, "join");
        assert_eq!(program.attach(&language, join, slot.clone()).unwrap(), None);
        let add = fresh(&mut program, &language, "add");
        let ejected = program.attach(&language, add, slot).unwrap().unwrap();
        assert_eq!(ejected.blocks[0].opcode, "join");

        let add = fresh(&mut program, &language, "add");
        assert_eq!(
            program.can_attach(&language, &add, &Target::After(ids[0])),
            Err(AttachError::WrongKind)
        );
    }

    fn scheme() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/scheme.ron"),
            &Validators::new(),
        )
        .unwrap()
    }

    fn items(program: &Program, id: BlockId, list: &str) -> Vec<String> {
        let block = program.find(id).unwrap();
        let items = block.lists.get(list).map(Vec::as_slice).unwrap_or(&[]);
        items
            .iter()
            .map(|item| match (&item.block, &item.literal) {
                (Some(inner), _) => inner.opcode.clone(),
                (None, Some(text)) => text.clone(),
                (None, None) => "_".into(),
            })
            .collect()
    }

    #[test]
    fn the_empty_slot_appends_and_nothing_lands_past_it() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["add"]);
        let at = |index| Target::Input {
            parent: ids[0],
            slot: Slot::item("args", index),
        };

        let car = fresh(&mut program, &language, "car");
        assert_eq!(
            program.can_attach(&language, &car, &at(1)),
            Err(AttachError::NoSuchInput {
                parent: ids[0],
                slot: Slot::item("args", 1)
            })
        );
        program.attach(&language, car, at(0)).unwrap();
        program.set_literal(ids[0], &Slot::item("args", 1), "2".into());
        let cdr = fresh(&mut program, &language, "cdr");
        program.attach(&language, cdr, at(2)).unwrap();
        assert_eq!(items(&program, ids[0], "args"), ["car", "2", "cdr"]);

        let list = fresh(&mut program, &language, "list");
        let ejected = program.attach(&language, list, at(0)).unwrap().unwrap();
        assert_eq!(ejected.blocks[0].opcode, "car");
        assert_eq!(items(&program, ids[0], "args"), ["list", "2", "cdr"]);

        let binding = fresh(&mut program, &language, "binding");
        assert!(matches!(
            program.can_attach(&language, &binding, &at(3)),
            Err(AttachError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn taking_an_item_out_leaves_a_hole_unless_it_was_last() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["add"]);
        let mut placed = Vec::new();
        for index in 0..3 {
            let car = fresh(&mut program, &language, "car");
            placed.push(car.blocks[0].id);
            let target = Target::Input {
                parent: ids[0],
                slot: Slot::item("args", index),
            };
            program.attach(&language, car, target).unwrap();
        }

        program.detach(placed[1]).unwrap();
        assert_eq!(items(&program, ids[0], "args"), ["car", "_", "car"]);
        program.remove(placed[2]).unwrap();
        assert_eq!(items(&program, ids[0], "args"), ["car"], "trailing holes go");
    }

    #[test]
    fn typing_at_the_empty_slot_appends_and_clearing_makes_a_hole() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["add"]);
        let item = |index| Slot::item("args", index);

        assert!(program.set_literal(ids[0], &item(0), "1".into()));
        assert!(program.set_literal(ids[0], &item(1), "2".into()));
        assert!(!program.set_literal(ids[0], &item(3), "4".into()), "past the empty slot");
        assert!(program.set_literal(ids[0], &item(2), "".into()), "nothing to append");
        assert_eq!(items(&program, ids[0], "args"), ["1", "2"]);

        program.set_literal(ids[0], &item(0), "".into());
        assert_eq!(items(&program, ids[0], "args"), ["_", "2"]);
        program.set_literal(ids[0], &item(1), "".into());
        assert!(items(&program, ids[0], "args").is_empty());
        let fresh = program.instantiate(&language, "add").unwrap();
        assert_eq!(program.find(ids[0]).unwrap().lists, fresh.lists, "no empty list is left behind");
    }

    #[test]
    fn an_item_moves_as_if_already_detached() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["add"]);
        program.set_literal(ids[0], &Slot::item("args", 0), "1".into());
        let car = fresh(&mut program, &language, "car");
        let car_id = car.blocks[0].id;
        let last = Target::Input {
            parent: ids[0],
            slot: Slot::item("args", 1),
        };
        program.attach(&language, car, last.clone()).unwrap();

        // Lifted, the last item leaves a trailing hole, so index 1 is the
        // empty slot again and 2 is past it.
        let run = program.run_at(car_id).unwrap();
        assert_eq!(program.can_move(&language, &run, &last), Ok(()));
        let past = Target::Input {
            parent: ids[0],
            slot: Slot::item("args", 2),
        };
        let mut detached = program.clone();
        detached.detach(car_id).unwrap();
        assert_eq!(program.can_move(&language, &run, &past), detached.can_attach(&language, &run, &past));
        assert!(program.can_move(&language, &run, &past).is_err());
    }

    #[test]
    fn an_item_goes_back_home_as_it_was() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["add"]);
        let mut placed = Vec::new();
        for index in 0..3 {
            let car = fresh(&mut program, &language, "car");
            placed.push(car.blocks[0].id);
            let target = Target::Input {
                parent: ids[0],
                slot: Slot::item("args", index),
            };
            program.attach(&language, car, target).unwrap();
        }
        let before = program.stacks.clone();
        for id in placed {
            let home = program.home_of(id).unwrap();
            let run = program.detach(id).unwrap();
            program.attach(&language, run, home).unwrap();
            assert_eq!(program.stacks, before, "{id:?}");
        }
    }

    #[test]
    fn loading_drops_trailing_holes() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["add"]);
        let args = vec![Input::default(), Input { literal: Some("1".into()), block: None }, Input::default()];
        program.find_mut(ids[0]).unwrap().lists.insert("args".into(), args);

        let back = Program::from_ron(&program.to_ron()).unwrap();
        assert_eq!(items(&back, ids[0], "args"), ["_", "1"]);
    }

    #[test]
    fn strict_types_refuse_what_loose_ones_convert() {
        for (language, allowed) in [(tiny(), true), (strict(), false)] {
            let mut program = Program::new(&language);
            let ids = stack(&mut program, &language, &["print"]);
            let add = fresh(&mut program, &language, "add");
            let slot = Target::Input {
                parent: ids[0],
                slot: Slot::input("value"),
            };
            assert_eq!(
                program.can_attach(&language, &add, &slot).is_ok(),
                allowed,
                "{}",
                language.name
            );
        }
    }

    #[test]
    fn detaching_a_reporter_leaves_the_literal_underneath() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["print"]);
        let join = fresh(&mut program, &language, "join");
        let join_id = join.blocks[0].id;
        let slot = Target::Input {
            parent: ids[0],
            slot: Slot::input("value"),
        };
        program.attach(&language, join, slot).unwrap();

        program.detach(join_id).unwrap();
        let input = &program.find(ids[0]).unwrap().inputs["value"];
        assert!(input.block.is_none());
        assert_eq!(input.literal.as_deref(), Some("Hello, world!"));
    }

    /// A just-loaded program, whose id counter has not been recovered yet.
    fn loaded(language: &Language, opcodes: &[&str]) -> (Program, Vec<BlockId>) {
        let mut program = Program::new(language);
        let ids = stack(&mut program, language, opcodes);
        (Program::from_ron(&program.to_ron()).unwrap(), ids)
    }

    #[test]
    fn ids_taken_out_of_the_tree_are_never_reissued() {
        let language = tiny();
        let (mut program, ids) = loaded(&language, &["print", "set"]);
        let _held = program.detach(ids[0]).unwrap();

        let next = program.fresh_id();
        assert!(ids.iter().all(|id| next.0 > id.0), "{next:?} reuses one of {ids:?}");
    }

    #[test]
    fn ids_of_removed_blocks_are_never_reissued() {
        let language = tiny();
        let (mut program, ids) = loaded(&language, &["print", "set"]);
        program.remove(ids[1]).unwrap();

        let next = program.fresh_id();
        assert!(next.0 > ids[1].0, "{next:?} reuses {:?}", ids[1]);
    }

    #[test]
    fn programs_nested_to_the_limit_through_list_items_load() {
        let language = scheme();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["list"]);
        let mut innermost = ids[0];
        for _ in 1..MAX_DEPTH {
            let inner = fresh(&mut program, &language, "list");
            let id = inner.blocks[0].id;
            let target = Target::Input {
                parent: innermost,
                slot: Slot::item("items", 0),
            };
            program.attach(&language, inner, target).unwrap();
            innermost = id;
        }
        assert_eq!(program.depth_of(innermost), Some(MAX_DEPTH));
        let back = Program::from_ron(&program.to_ron()).unwrap();
        assert_eq!(back.depth_of(innermost), Some(MAX_DEPTH));
    }

    #[test]
    fn nothing_attaches_past_the_depth_limit() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["while"]);
        let mut innermost = ids[0];
        for _ in 1..MAX_DEPTH {
            let inner = fresh(&mut program, &language, "while");
            let id = inner.blocks[0].id;
            let target = Target::BranchStart {
                parent: innermost,
                branch: "body".into(),
            };
            program.attach(&language, inner, target).unwrap();
            innermost = id;
        }
        assert_eq!(program.depth_of(innermost), Some(MAX_DEPTH));
        // RON's own recursion limit must not cut in before ours.
        let back = Program::from_ron(&program.to_ron()).unwrap();
        assert_eq!(back.depth_of(innermost), Some(MAX_DEPTH));

        let one_more = fresh(&mut program, &language, "print");
        let target = Target::BranchStart {
            parent: innermost,
            branch: "body".into(),
        };
        assert_eq!(
            program.can_attach(&language, &one_more, &target),
            Err(AttachError::TooDeep {
                depth: MAX_DEPTH + 1
            })
        );
    }

    #[test]
    fn a_run_goes_back_home_as_it_was() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["when_run", "while", "print"]);
        let other = stack(&mut program, &language, &["print"]);
        let inner = program.instantiate(&language, "set").unwrap();
        let join = program.instantiate(&language, "join").unwrap();
        let (inner_id, join_id) = (inner.id, join.id);
        let owner = program.find_mut(ids[1]).unwrap();
        owner.branches.get_mut("body").unwrap().push(inner);
        program.find_mut(ids[2]).unwrap().inputs.get_mut("value").unwrap().block = Some(Box::new(join));
        let before = program.stacks.clone();

        let sorted = |stacks: &[Stack]| {
            let mut stacks = stacks.to_vec();
            stacks.sort_by_key(|stack| stack.blocks[0].id);
            stacks
        };
        for id in [ids[0], ids[1], ids[2], inner_id, join_id, other[0]] {
            let home = program.home_of(id).unwrap();
            let run = program.detach(id).unwrap();
            program.attach(&language, run, home).unwrap();
            assert_eq!(sorted(&program.stacks), sorted(&before), "{id:?} did not go back as it was");
        }
    }

    #[test]
    fn remove_closes_the_gap() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["when_run", "print", "set"]);

        program.remove(ids[1]).unwrap();
        assert_eq!(opcodes(&program.stacks[0].blocks), ["when_run", "set"]);
    }

    #[test]
    fn a_run_copied_in_place_is_what_detaching_takes() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["when_run", "while", "print"]);
        let inner = program.instantiate(&language, "set").unwrap();
        let join = program.instantiate(&language, "join").unwrap();
        let (inner_id, join_id) = (inner.id, join.id);
        program.find_mut(ids[1]).unwrap().branches.get_mut("body").unwrap().push(inner);
        program.find_mut(ids[2]).unwrap().inputs.get_mut("value").unwrap().block = Some(Box::new(join));

        for id in [ids[0], ids[1], ids[2], inner_id, join_id] {
            assert_eq!(program.run_at(id), program.clone().detach(id), "{id:?}");
        }
        assert_eq!(program.run_at(BlockId(9999)), None);
    }

    #[test]
    fn duplicates_get_fresh_ids_throughout() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["print", "set"]);

        let copy = program.duplicate(ids[0]).unwrap();
        assert_eq!(opcodes(&copy.blocks), ["print", "set"]);
        assert!(copy.blocks.iter().all(|block| !ids.contains(&block.id)));
    }

    #[test]
    fn programs_round_trip_through_ron_with_literals_as_typed() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["when_run", "print"]);
        program.set_literal(ids[1], &Slot::input("value"), "  12.3e ".into());

        let text = program.to_ron();
        let back = Program::from_ron(&text).unwrap();
        assert_eq!(back.stacks, program.stacks);
        assert_eq!(
            back.find(ids[1]).unwrap().inputs["value"].literal.as_deref(),
            Some("  12.3e ")
        );
    }
}
