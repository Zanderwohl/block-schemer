use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::edit::Location;
use crate::language::{BlockDef, Language, Part, ron_options};

pub const FORMAT_VERSION: u32 = 3;

/// Deepest nesting allowed, counting a stack's own blocks as depth 1 and each
/// branch or input as one more. Deeper blocks load as `TooDeep` problems and
/// cannot be attached. Kept under 128 to leave room for later nesting.
pub const MAX_DEPTH: usize = 120;

/// RON's recursion limit for program loads. Measured: a level of nesting
/// costs RON 6 through a branch, 7 through an input and 9 through a list
/// item, plus about 8 for the file around it, so its default of 128 stops
/// loads near depth 13. Only a file nested past this is a fatal syntax error.
pub const RON_RECURSION_LIMIT: usize = MAX_DEPTH * 9 + 16;

/// Only stack positions are stored; block positions are derived, so a language
/// whose labels change width re-flows old files instead of overlapping them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Program {
    /// Lets a host check or pick the language before a full load.
    pub language: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub stacks: Vec<Stack>,
    /// Recovered from the tree on first use, then monotonic, so ids of blocks
    /// out of the tree (a run in hand) are never reissued.
    #[serde(skip)]
    next_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    /// Head's top-left, canvas units at zoom 1.
    pub pos: [f32; 2],
    /// Head first. Never empty. A lone reporter is a stack of one.
    pub blocks: Vec<Block>,
}

/// Stable across edits, drags, saves and loads. The one handle AST nodes,
/// problems, pauses and breakpoints use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(pub u64);

/// Inputs and branches are keyed by name, not spec position, so old files
/// survive a language adding or reordering them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: BlockId,
    pub opcode: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    /// An item holding neither reporter nor literal is a hole, kept so the
    /// items after it keep their places.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lists: BTreeMap<String, Vec<Input>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub branches: BTreeMap<String, Vec<Block>>,
    /// Makes this a reference to a declared name: its one input mirrors the
    /// declaration's literal and cannot be edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refers: Option<Declaration>,
}

/// A literal in a slot a scope `declares`: where a name is introduced.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Declaration {
    pub block: BlockId,
    pub slot: Slot,
}

/// A slot of a block: a single input, or one item of a list. Index `len` of
/// a list is the empty slot after its items, which appends.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Slot {
    pub input: String,
    /// `None` for a single input.
    pub item: Option<usize>,
}

impl Slot {
    pub fn input(name: impl Into<String>) -> Self {
        Self {
            input: name.into(),
            item: None,
        }
    }

    pub fn item(list: impl Into<String>, index: usize) -> Self {
        Self {
            input: list.into(),
            item: Some(index),
        }
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.item {
            Some(index) => write!(f, "{}[{index}]", self.input),
            None => write!(f, "{}", self.input),
        }
    }
}

/// The literal stays under a plugged reporter and returns when it is removed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Input {
    /// As typed, valid or not; parsed when the AST is built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<Box<Block>>,
}

impl Input {
    pub fn is_hole(&self) -> bool {
        self.literal.is_none() && self.block.is_none()
    }
}

#[derive(Debug)]
pub enum ProgramError {
    Io(std::io::Error),
    /// Unreadable RON: the only unrecoverable load failure.
    Syntax(ron::error::SpannedError),
}

/// Loaded anyway.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadWarning {
    Extension { expected: String, found: Option<String> },
    Language { expected: String, found: String },
    Version { found: u32 },
}

impl Program {
    pub fn new(language: &Language) -> Self {
        Self {
            language: language.name.clone(),
            version: FORMAT_VERSION,
            stacks: Vec::new(),
            next_id: 0,
        }
    }

    /// Trailing holes in lists are dropped.
    pub fn from_ron(text: &str) -> Result<Self, ProgramError> {
        let mut program: Self = with_deep_stack(|| {
            ron_options()
                .with_recursion_limit(RON_RECURSION_LIMIT)
                .from_str(text)
                .map_err(ProgramError::Syntax)
        })?;
        for stack in &mut program.stacks {
            for block in &mut stack.blocks {
                block.trim_all();
            }
        }
        program.sync_references();
        Ok(program)
    }

    pub fn to_ron(&self) -> String {
        let config = ron::ser::PrettyConfig::new()
            .extensions(ron::extensions::Extensions::IMPLICIT_SOME);
        with_deep_stack(|| {
            ron::Options::default()
                .with_recursion_limit(RON_RECURSION_LIMIT)
                .to_string_pretty(self, config)
                .expect("a program is plain data and always serializes")
        })
    }

    /// Reads a program, noting anything odd about it without refusing it.
    pub fn load(
        path: impl AsRef<Path>,
        language: &Language,
    ) -> Result<(Self, Vec<LoadWarning>), ProgramError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(ProgramError::Io)?;
        let program = Self::from_ron(&text)?;

        let mut warnings = Vec::new();
        let found = path.extension().map(|ext| ext.to_string_lossy().into_owned());
        if found.as_deref() != Some(language.file.extension.as_str()) {
            warnings.push(LoadWarning::Extension {
                expected: language.file.extension.clone(),
                found,
            });
        }
        if program.language != language.name {
            warnings.push(LoadWarning::Language {
                expected: language.name.clone(),
                found: program.language.clone(),
            });
        }
        if program.version > FORMAT_VERSION {
            warnings.push(LoadWarning::Version {
                found: program.version,
            });
        }
        Ok((program, warnings))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_ron())
    }

    /// An id no block in the program, or taken out of it, has had.
    pub fn fresh_id(&mut self) -> BlockId {
        let id = self.next_id.max(self.max_id() + 1);
        self.next_id = id + 1;
        BlockId(id)
    }

    /// Raises the id counter past `blocks`, which are about to leave the tree.
    pub(crate) fn reserve_ids(&mut self, blocks: &[Block]) {
        let mut highest = 0;
        walk(blocks, &mut |block| highest = highest.max(block.id.0));
        self.next_id = self.next_id.max(highest + 1);
    }

    fn max_id(&self) -> u64 {
        let mut highest = 0;
        for stack in &self.stacks {
            walk(&stack.blocks, &mut |block| highest = highest.max(block.id.0));
        }
        highest
    }

    /// A fresh block of `opcode` with its default literals and empty branches.
    pub fn instantiate(&mut self, language: &Language, opcode: &str) -> Option<Block> {
        let def = language.block(opcode)?;
        let mut block = Block {
            id: self.fresh_id(),
            opcode: opcode.to_owned(),
            inputs: BTreeMap::new(),
            lists: BTreeMap::new(),
            branches: BTreeMap::new(),
            refers: None,
        };
        for part in &def.parts {
            match part {
                Part::Input(input) => {
                    block.inputs.insert(
                        input.name.clone(),
                        Input {
                            literal: input.default.clone(),
                            block: None,
                        },
                    );
                }
                Part::Branch(name) => {
                    block.branches.insert(name.clone(), Vec::new());
                }
                Part::List(_) | Part::Label(_) => {}
            }
        }
        Some(block)
    }

    /// Every block, parents first.
    pub fn each_block<'a>(&'a self, mut visit: impl FnMut(&'a Block)) {
        for stack in &self.stacks {
            walk(&stack.blocks, &mut visit);
        }
    }

    pub fn find(&self, id: BlockId) -> Option<&Block> {
        self.stacks.iter().find_map(|stack| find_in(&stack.blocks, id))
    }

    pub fn find_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.stacks
            .iter_mut()
            .find_map(|stack| find_in_mut(&mut stack.blocks, id))
    }

    pub fn locate(&self, id: BlockId) -> Option<Location> {
        self.stacks.iter().enumerate().find_map(|(index, stack)| {
            match stack.blocks.iter().position(|block| block.id == id) {
                Some(position) => Some(Location::Stack {
                    stack: index,
                    index: position,
                }),
                None => stack.blocks.iter().find_map(|block| locate_in(block, id)),
            }
        })
    }

    /// 1 for a block at the top level of a stack, one more per branch or input.
    pub fn depth_of(&self, id: BlockId) -> Option<usize> {
        self.stacks
            .iter()
            .find_map(|stack| depth_in(&stack.blocks, id, 1))
    }

    /// False if the block is gone, is a reference, or the slot is past a
    /// list's empty slot. Emptying an item's text makes it a hole, so a field
    /// keeps one address as its item comes and goes. References to the slot
    /// follow the new text.
    pub fn set_literal(&mut self, block: BlockId, slot: &Slot, text: String) -> bool {
        let declaration = Declaration {
            block,
            slot: slot.clone(),
        };
        let Some(block) = self.find_mut(block).filter(|block| block.refers.is_none()) else {
            return false;
        };
        let Some(index) = slot.item else {
            block.inputs.entry(slot.input.clone()).or_default().literal = Some(text.clone());
            self.rename(&declaration, &text);
            return true;
        };
        let len = block.lists.get(&slot.input).map_or(0, Vec::len);
        if index > len {
            return false;
        }
        if index == len && text.is_empty() {
            return true;
        }
        let items = block.lists.entry(slot.input.clone()).or_default();
        if index == len {
            items.push(Input::default());
        }
        items[index].literal = (!text.is_empty()).then(|| text.clone());
        block.trim_lists();
        self.rename(&declaration, &text);
        true
    }

    /// The literal a declaration names, if it is still there. An item emptied
    /// out of a list's end has none, though references keep its index.
    pub fn declared_name(&self, declaration: &Declaration) -> Option<&str> {
        let input = self.find(declaration.block)?.slot(&declaration.slot)?;
        input.block.is_none().then_some(input.literal.as_deref()).flatten()
    }

    fn rename(&mut self, declaration: &Declaration, name: &str) {
        for stack in &mut self.stacks {
            walk_mut(&mut stack.blocks, &mut |block| {
                if block.refers.as_ref() == Some(declaration) {
                    block.set_reference_name(name);
                }
            });
        }
    }

    /// Brings every reference's name in line with its declaration, as a file
    /// edited by hand may not be. A reference keeps its last name while its
    /// declaring block is gone or a block covers the name.
    fn sync_references(&mut self) {
        let mut names = BTreeMap::new();
        for stack in &self.stacks {
            walk(&stack.blocks, &mut |block| {
                if let Some(declaration) = &block.refers {
                    names.insert(declaration.clone(), None);
                }
            });
        }
        for (declaration, name) in &mut names {
            let slot = self.find(declaration.block).map(|block| block.slot(&declaration.slot));
            if let Some(slot) = slot
                && slot.is_none_or(|input| input.block.is_none())
            {
                *name = Some(self.declared_name(declaration).unwrap_or_default().to_owned());
            }
        }
        for stack in &mut self.stacks {
            walk_mut(&mut stack.blocks, &mut |block| {
                if let Some(Some(name)) = block.refers.as_ref().and_then(|declaration| names.get(declaration)) {
                    block.set_reference_name(name);
                }
            });
        }
    }
}

impl Block {
    /// A reference block has one input, which holds the name.
    pub(crate) fn set_reference_name(&mut self, name: &str) {
        for input in self.inputs.values_mut() {
            input.literal = Some(name.to_owned());
        }
    }

    /// The blocks whose local names this block's scope covers: itself, and
    /// those plugged into its declaring slots that hand their names on.
    pub fn scope_blocks(&self, language: &Language) -> Vec<BlockId> {
        let mut found = vec![self.id];
        self.handed_on(language, &mut found);
        found
    }

    fn handed_on(&self, language: &Language, found: &mut Vec<BlockId>) {
        let Some(def) = language.block(&self.opcode) else { return };
        let singles = self.inputs.iter().map(|(name, input)| (name, std::slice::from_ref(input)));
        let lists = self.lists.iter().map(|(name, items)| (name, items.as_slice()));
        for (_, inputs) in singles.chain(lists).filter(|(name, _)| def.declares(name)) {
            for inner in inputs.iter().filter_map(|input| input.block.as_deref()) {
                if language.block(&inner.opcode).is_some_and(BlockDef::hands_on) {
                    found.push(inner.id);
                    inner.handed_on(language, found);
                }
            }
        }
    }

    pub fn slot(&self, slot: &Slot) -> Option<&Input> {
        match slot.item {
            None => self.inputs.get(&slot.input),
            Some(index) => self.lists.get(&slot.input)?.get(index),
        }
    }

    /// An item past a list's end is created at the empty slot, never beyond.
    pub(crate) fn slot_entry(&mut self, slot: &Slot) -> Option<&mut Input> {
        match slot.item {
            None => Some(self.inputs.entry(slot.input.clone()).or_default()),
            Some(index) => {
                if index > self.lists.get(&slot.input).map_or(0, Vec::len) {
                    return None;
                }
                let items = self.lists.entry(slot.input.clone()).or_default();
                if index == items.len() {
                    items.push(Input::default());
                }
                items.get_mut(index)
            }
        }
    }

    /// How many items `list` shows once the reporter `lifted` is out of it:
    /// trailing holes, including the one it may leave, do not count.
    pub fn list_len(&self, list: &str, lifted: Option<BlockId>) -> usize {
        let items = self.lists.get(list).map(Vec::as_slice).unwrap_or(&[]);
        let empty = |input: &Input| {
            input.literal.is_none() && input.block.as_ref().is_none_or(|inner| Some(inner.id) == lifted)
        };
        items.len() - items.iter().rev().take_while(|input| empty(input)).count()
    }

    /// Drops trailing holes, and lists left empty, which a fresh block has
    /// none of.
    pub(crate) fn trim_lists(&mut self) {
        for items in self.lists.values_mut() {
            while items.last().is_some_and(Input::is_hole) {
                items.pop();
            }
        }
        self.lists.retain(|_, items| !items.is_empty());
    }

    fn trim_all(&mut self) {
        self.trim_lists();
        let inputs = self.inputs.values_mut().chain(self.lists.values_mut().flatten());
        for inner in inputs.filter_map(|input| input.block.as_deref_mut()) {
            inner.trim_all();
        }
        for child in self.branches.values_mut().flatten() {
            child.trim_all();
        }
    }

    /// Reporters in single inputs and list items alike.
    pub fn reporters(&self) -> impl Iterator<Item = &Block> {
        self.inputs
            .values()
            .chain(self.lists.values().flatten())
            .filter_map(|input| input.block.as_deref())
    }

    /// Nesting levels this block spans, counting itself.
    pub fn height(&self) -> usize {
        let inputs = self.reporters().map(Block::height);
        let branches = self.branches.values().flatten().map(Block::height);
        1 + inputs.chain(branches).max().unwrap_or(0)
    }
}

/// Stack for RON work on a `MAX_DEPTH` program. Measured: debug builds need
/// between 2 and 4 MiB, more than spawned threads, tests and async runtimes
/// usually get.
const RON_STACK: usize = 16 * 1024 * 1024;

/// Runs `work` on a thread with `RON_STACK` of stack, so loads and saves are
/// safe from whatever thread calls them. Inline where threads are missing.
fn with_deep_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    #[cfg(not(target_family = "wasm"))]
    {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(RON_STACK)
                .spawn_scoped(scope, work)
                .expect("a thread for RON work")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
    #[cfg(target_family = "wasm")]
    {
        work()
    }
}

pub(crate) fn walk_mut(blocks: &mut [Block], visit: &mut impl FnMut(&mut Block)) {
    for block in blocks {
        visit(block);
        let inputs = block.inputs.values_mut().chain(block.lists.values_mut().flatten());
        for inner in inputs.filter_map(|input| input.block.as_deref_mut()) {
            walk_mut(std::slice::from_mut(inner), visit);
        }
        for branch in block.branches.values_mut() {
            walk_mut(branch, visit);
        }
    }
}

/// Every block in `blocks` and everything nested in them, parents first.
pub(crate) fn walk<'a>(blocks: &'a [Block], visit: &mut impl FnMut(&'a Block)) {
    for block in blocks {
        visit(block);
        for inner in block.reporters() {
            walk(std::slice::from_ref(inner), visit);
        }
        for branch in block.branches.values() {
            walk(branch, visit);
        }
    }
}

pub(crate) fn find_in(blocks: &[Block], id: BlockId) -> Option<&Block> {
    blocks.iter().find_map(|block| find_block(block, id))
}

fn find_block(block: &Block, id: BlockId) -> Option<&Block> {
    if block.id == id {
        return Some(block);
    }
    block
        .reporters()
        .find_map(|inner| find_block(inner, id))
        .or_else(|| block.branches.values().find_map(|seq| find_in(seq, id)))
}

fn find_in_mut(blocks: &mut [Block], id: BlockId) -> Option<&mut Block> {
    blocks.iter_mut().find_map(|block| find_block_mut(block, id))
}

fn find_block_mut(block: &mut Block, id: BlockId) -> Option<&mut Block> {
    if block.id == id {
        return Some(block);
    }
    // Fields borrowed apart: borrowing all of `block` here would outlive the
    // early return.
    let inputs = block.inputs.values_mut().chain(block.lists.values_mut().flatten());
    for inner in inputs.filter_map(|input| input.block.as_deref_mut()) {
        if let Some(found) = find_block_mut(inner, id) {
            return Some(found);
        }
    }
    block
        .branches
        .values_mut()
        .find_map(|seq| find_in_mut(seq, id))
}

fn locate_in(block: &Block, id: BlockId) -> Option<Location> {
    let singles = block.inputs.iter().map(|(name, input)| (Slot::input(name.clone()), input));
    let items = block.lists.iter().flat_map(|(name, items)| {
        items
            .iter()
            .enumerate()
            .map(|(index, input)| (Slot::item(name.clone(), index), input))
    });
    for (slot, input) in singles.chain(items) {
        if let Some(inner) = input.block.as_deref() {
            if inner.id == id {
                return Some(Location::Input {
                    parent: block.id,
                    slot,
                });
            }
            if let Some(found) = locate_in(inner, id) {
                return Some(found);
            }
        }
    }
    for (name, seq) in &block.branches {
        for (index, child) in seq.iter().enumerate() {
            if child.id == id {
                return Some(Location::Branch {
                    parent: block.id,
                    branch: name.clone(),
                    index,
                });
            }
            if let Some(found) = locate_in(child, id) {
                return Some(found);
            }
        }
    }
    None
}

fn depth_in(blocks: &[Block], id: BlockId, depth: usize) -> Option<usize> {
    blocks.iter().find_map(|block| {
        if block.id == id {
            return Some(depth);
        }
        block
            .reporters()
            .find_map(|inner| depth_in(std::slice::from_ref(inner), id, depth + 1))
            .or_else(|| {
                block
                    .branches
                    .values()
                    .find_map(|seq| depth_in(seq, id, depth + 1))
            })
    })
}

impl std::fmt::Display for ProgramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Syntax(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProgramError {}

impl std::fmt::Display for LoadWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extension { expected, found } => match found {
                Some(found) => write!(f, "expected a .{expected} file, found .{found}"),
                None => write!(f, "expected a .{expected} file"),
            },
            Self::Language { expected, found } => {
                write!(f, "written for language `{found}`, not `{expected}`")
            }
            Self::Version { found } => write!(
                f,
                "written by format version {found}; this reader knows {FORMAT_VERSION}"
            ),
        }
    }
}
