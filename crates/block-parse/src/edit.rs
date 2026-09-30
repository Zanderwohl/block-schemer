//! Tree operations, addressed by [`BlockId`] because positions go stale.

use crate::program::{Block, BlockId};

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
