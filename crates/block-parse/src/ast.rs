//! Always a whole tree. A fault is a [`Problem`] where the fault is, keeping
//! what could be parsed beneath it, so a consumer can compile around it.
//! Nodes carry their [`BlockId`] as the way back to the canvas.

use serde::{Deserialize, Serialize};

use crate::program::BlockId;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ast {
    /// One per stack, hat-headed or not.
    pub scripts: Vec<Script>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Node(Node),
    Problem(Problem),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Literal(Value),
    Node(Box<Node>),
    Problem(Box<Problem>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: BlockId,
    pub opcode: String,
    /// Spec order.
    pub args: Vec<Arg>,
    /// Spec order.
    pub branches: Vec<Branch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arg {
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub block: Option<BlockId>,
    /// An empty slot has no block of its own: the owner and the input name.
    pub slot: Option<(BlockId, String)>,
    pub code: ProblemCode,
    pub severity: Severity,
    pub message: String,
    /// The offending block as far as it parsed, so valid children survive.
    pub recovered: Option<Box<Node>>,
}

/// Stable for matching on, unlike [`Problem::message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProblemCode {
    UnknownOpcode,
    /// Neither a reporter nor a literal.
    MissingInput,
    TypeMismatch,
    /// A literal the slot's type cannot hold; only a hand-edited file has one.
    InvalidLiteral,
    UnknownInput,
    UnknownBranch,
    ReporterAsStatement,
    StatementAsInput,
    HatNotAtTop,
    AfterCap,
    /// Reported on the second block.
    DuplicateId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Warning,
    Error,
}
