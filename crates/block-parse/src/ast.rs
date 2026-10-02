//! Always a whole tree. A fault is a [`Problem`] where the fault is, keeping
//! what could be parsed beneath it, so a consumer can compile around it.
//! Nodes carry their [`BlockId`] as the way back to the canvas.

use serde::{Deserialize, Serialize};

use crate::program::{BlockId, Declaration, Slot};
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
    /// A reporter in a slot of another type, allowed by `accepts` or `fits`.
    /// Never produced for an exact match. What conversion means is the
    /// consumer's call.
    Convert {
        from: String,
        to: String,
        value: Box<Expr>,
    },
    Problem(Box<Problem>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: BlockId,
    pub opcode: String,
    /// Spec order.
    pub args: Vec<Arg>,
    /// Spec order.
    pub lists: Vec<List>,
    /// Spec order.
    pub branches: Vec<Branch>,
    /// For a reference, the declaration it names. Always in scope: one
    /// that is not is a problem instead.
    pub refers: Option<Declaration>,
    /// A callable block left as its name, not called. It has no args or
    /// lists but a procedure reference's name. A call leaves out the
    /// parameters it hides.
    #[serde(default)]
    pub named: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arg {
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct List {
    pub name: String,
    /// Holes are `MissingInput` problems, so indices match the program's.
    pub items: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub block: Option<BlockId>,
    /// An empty slot has no block of its own: the owner and the slot.
    pub slot: Option<(BlockId, Slot)>,
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
    /// Neither a reporter nor a literal; also a hole in a list.
    MissingInput,
    /// A list with fewer items than its spec asks for (`+`). On the block.
    TooFewItems,
    TypeMismatch,
    /// Rejected by the slot type's validator; the message is the validator's.
    InvalidLiteral,
    UnknownInput,
    UnknownBranch,
    ReporterAsStatement,
    StatementAsInput,
    HatNotAtTop,
    AfterCap,
    /// Reported on the second block.
    DuplicateId,
    /// A reference outside its declaration's scope, or to one that is gone.
    /// Nothing is recovered: it reads as an empty slot.
    OutOfScope,
    /// A declaring slot left blank, or a reference to one. No language
    /// takes an empty name.
    Unnamed,
    /// A call showing fewer of its procedure's required parameters, or
    /// more, than it has, in a language not `curried`. On the block.
    Arity,
    /// Nested past `MAX_DEPTH`. Replaces the first block past the limit; nothing
    /// below it is parsed.
    TooDeep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Warning,
    Error,
}

impl Ast {
    /// Every problem in tree order, including those under recovered nodes.
    pub fn problems(&self) -> Vec<&Problem> {
        let mut found = Vec::new();
        for script in &self.scripts {
            statements(&script.body, &mut found);
        }
        found
    }

    /// No error-level problems. Warnings are allowed.
    pub fn is_clean(&self) -> bool {
        self.problems()
            .iter()
            .all(|problem| problem.severity < Severity::Error)
    }
}

impl Node {
    pub fn arg(&self, name: &str) -> Option<&Expr> {
        self.args.iter().find(|arg| arg.name == name).map(|arg| &arg.value)
    }

    pub fn list(&self, name: &str) -> Option<&[Expr]> {
        self.lists
            .iter()
            .find(|list| list.name == name)
            .map(|list| list.items.as_slice())
    }

    pub fn branch(&self, name: &str) -> Option<&[Stmt]> {
        self.branches
            .iter()
            .find(|branch| branch.name == name)
            .map(|branch| branch.body.as_slice())
    }
}

fn statements<'a>(body: &'a [Stmt], found: &mut Vec<&'a Problem>) {
    for statement in body {
        match statement {
            Stmt::Node(node) => node_problems(node, found),
            Stmt::Problem(problem) => problem_and_below(problem, found),
        }
    }
}

fn node_problems<'a>(node: &'a Node, found: &mut Vec<&'a Problem>) {
    for arg in &node.args {
        expression(&arg.value, found);
    }
    for item in node.lists.iter().flat_map(|list| &list.items) {
        expression(item, found);
    }
    for branch in &node.branches {
        statements(&branch.body, found);
    }
}

fn expression<'a>(expr: &'a Expr, found: &mut Vec<&'a Problem>) {
    match expr {
        Expr::Literal(_) => {}
        Expr::Node(node) => node_problems(node, found),
        Expr::Convert { value, .. } => expression(value, found),
        Expr::Problem(problem) => problem_and_below(problem, found),
    }
}

fn problem_and_below<'a>(problem: &'a Problem, found: &mut Vec<&'a Problem>) {
    found.push(problem);
    if let Some(node) = &problem.recovered {
        node_problems(node, found);
    }
}
