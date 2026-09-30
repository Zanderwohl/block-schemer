//! Tree operations, addressed by [`BlockId`] because positions go stale.

use crate::language::{BlockKind, Fit, Language};
use crate::program::{Block, BlockId, MAX_DEPTH, Program, Stack};

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
    Input { parent: BlockId, input: String },
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
    /// Ejects any reporter already there.
    Input { parent: BlockId, input: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttachError {
    NoSuchBlock(BlockId),
    NoSuchBranch { parent: BlockId, branch: String },
    NoSuchInput { parent: BlockId, input: String },
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
        let Some(head) = fragment.blocks.first() else {
            return Err(AttachError::WrongKind);
        };
        if matches!(target, Target::Free { .. }) {
            return Ok(());
        }
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
            Target::Input { parent, input } => {
                let output = def.kind.output().ok_or(AttachError::WrongKind)?;
                if fragment.blocks.len() != 1 {
                    return Err(AttachError::WrongKind);
                }
                let owner = self.find(*parent).ok_or(AttachError::NoSuchBlock(*parent))?;
                let slot = language
                    .block(&owner.opcode)
                    .and_then(|def| def.input(input))
                    .ok_or_else(|| AttachError::NoSuchInput {
                        parent: *parent,
                        input: input.clone(),
                    })?;
                if language.fit(output, &slot.ty) == Fit::No {
                    return Err(AttachError::TypeMismatch {
                        slot: slot.ty.clone(),
                        output: output.to_owned(),
                    });
                }
                self.depth_of(*parent).unwrap_or(1) + 1
            }
            Target::After(id) => {
                statement()?;
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
                if ends_in_cap && index + 1 < seq.len() {
                    return Err(AttachError::CapWouldOrphan);
                }
                self.depth_of(*id).unwrap_or(1)
            }
            Target::BranchStart { parent, branch } => {
                statement()?;
                let owner = self.find(*parent).ok_or(AttachError::NoSuchBlock(*parent))?;
                let has_branch = language
                    .block(&owner.opcode)
                    .is_some_and(|def| def.has_branch(branch));
                if !has_branch {
                    return Err(AttachError::NoSuchBranch {
                        parent: *parent,
                        branch: branch.clone(),
                    });
                }
                if ends_in_cap && owner.branches.get(branch).is_some_and(|seq| !seq.is_empty()) {
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
            Target::Input { parent, input } => {
                let owner = self.find_mut(parent).expect("checked by can_attach");
                let reporter = blocks.into_iter().next().expect("checked by can_attach");
                let slot = owner.inputs.entry(input).or_default();
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
        let mut blocks = match self.sequence_of(id) {
            Some((seq, index)) => seq[index..].to_vec(),
            None => vec![self.find(id)?.clone()],
        };
        for block in &mut blocks {
            self.renumber(block);
        }
        Some(Fragment { blocks })
    }

    fn renumber(&mut self, block: &mut Block) {
        block.id = self.fresh_id();
        for input in block.inputs.values_mut() {
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
            Location::Input { parent, input } => *self
                .find_mut(parent)?
                .inputs
                .get_mut(&input)?
                .block
                .take()?,
            Location::Stack { .. } | Location::Branch { .. } => {
                let (seq, index) = self.sequence_of_mut(id)?;
                seq.remove(index)
            }
        };
        self.stacks.retain(|stack| !stack.blocks.is_empty());
        self.reserve_ids(std::slice::from_ref(&removed));
        Some(removed)
    }

    /// Where a run detached at `id` would go to be put back as it was: after
    /// the block above it, at the start of its branch, into its slot, or as
    /// its whole stack at its old position. A whole stack comes back last in
    /// stack order, which changes drawing and script order but nothing else.
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
            Location::Input { parent, input } => Target::Input { parent, input },
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
    for input in block.inputs.values_mut() {
        if input.block.as_ref().is_some_and(|inner| inner.id == id) {
            return input.block.take().map(|inner| vec![*inner]);
        }
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
        let inputs = block.inputs.values().filter_map(|input| input.block.as_deref());
        inputs
            .into_iter()
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
    for input in block.inputs.values_mut() {
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

    #[test]
    fn reporters_go_in_slots_and_eject_what_was_there() {
        let language = tiny();
        let mut program = Program::new(&language);
        let ids = stack(&mut program, &language, &["print"]);
        let slot = Target::Input {
            parent: ids[0],
            input: "value".into(),
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

    #[test]
    fn strict_types_refuse_what_loose_ones_convert() {
        for (language, allowed) in [(tiny(), true), (strict(), false)] {
            let mut program = Program::new(&language);
            let ids = stack(&mut program, &language, &["print"]);
            let add = fresh(&mut program, &language, "add");
            let slot = Target::Input {
                parent: ids[0],
                input: "value".into(),
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
            input: "value".into(),
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
        program.set_literal(ids[1], "value", "  12.3e ".into());

        let text = program.to_ron();
        let back = Program::from_ron(&text).unwrap();
        assert_eq!(back.stacks, program.stacks);
        assert_eq!(
            back.find(ids[1]).unwrap().inputs["value"].literal.as_deref(),
            Some("  12.3e ")
        );
    }
}
