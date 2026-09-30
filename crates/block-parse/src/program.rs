use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::edit::Location;
use crate::language::{Language, Part, ron_options};

pub const FORMAT_VERSION: u32 = 1;

/// Deepest nesting allowed, counting a stack's own blocks as depth 1 and each
/// branch or input as one more. Deeper blocks load as `TooDeep` problems and
/// cannot be attached. Kept under 128 to leave room for later nesting.
pub const MAX_DEPTH: usize = 120;

/// RON's recursion limit for program loads. Measured: a level of nesting
/// costs RON 6 through a branch and 7 through an input, plus about 8 for the
/// file around it, so its default of 128 stops loads near depth 17. Only a
/// file nested past this is a fatal syntax error.
pub const RON_RECURSION_LIMIT: usize = MAX_DEPTH * 7 + 16;

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub branches: BTreeMap<String, Vec<Block>>,
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
    /// Newer than this crate's format.
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

    pub fn from_ron(text: &str) -> Result<Self, ProgramError> {
        with_deep_stack(|| {
            ron_options()
                .with_recursion_limit(RON_RECURSION_LIMIT)
                .from_str(text)
                .map_err(ProgramError::Syntax)
        })
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
            branches: BTreeMap::new(),
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
                Part::Label(_) => {}
            }
        }
        Some(block)
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

    /// Writes typed text into a slot. False if the block is gone.
    pub fn set_literal(&mut self, block: BlockId, input: &str, text: String) -> bool {
        let Some(block) = self.find_mut(block) else {
            return false;
        };
        block.inputs.entry(input.to_owned()).or_default().literal = Some(text);
        true
    }
}

impl Block {
    /// Nesting levels this block spans, counting itself.
    pub fn height(&self) -> usize {
        let inputs = self
            .inputs
            .values()
            .filter_map(|input| input.block.as_deref())
            .map(Block::height);
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

/// Every block in `blocks` and everything nested in them, parents first.
pub(crate) fn walk<'a>(blocks: &'a [Block], visit: &mut impl FnMut(&'a Block)) {
    for block in blocks {
        visit(block);
        for input in block.inputs.values() {
            if let Some(inner) = input.block.as_deref() {
                walk(std::slice::from_ref(inner), visit);
            }
        }
        for branch in block.branches.values() {
            walk(branch, visit);
        }
    }
}

fn find_in(blocks: &[Block], id: BlockId) -> Option<&Block> {
    blocks.iter().find_map(|block| find_block(block, id))
}

fn find_block(block: &Block, id: BlockId) -> Option<&Block> {
    if block.id == id {
        return Some(block);
    }
    block
        .inputs
        .values()
        .filter_map(|input| input.block.as_deref())
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
    for input in block.inputs.values_mut() {
        if let Some(inner) = input.block.as_deref_mut()
            && let Some(found) = find_block_mut(inner, id)
        {
            return Some(found);
        }
    }
    block
        .branches
        .values_mut()
        .find_map(|seq| find_in_mut(seq, id))
}

fn locate_in(block: &Block, id: BlockId) -> Option<Location> {
    for (name, input) in &block.inputs {
        if let Some(inner) = input.block.as_deref() {
            if inner.id == id {
                return Some(Location::Input {
                    parent: block.id,
                    input: name.clone(),
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
        let inputs = block.inputs.values().filter_map(|input| input.block.as_deref());
        inputs
            .into_iter()
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
