use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

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
