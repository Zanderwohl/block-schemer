//! Language definitions, programs and ASTs for block-based editors.
//!
//! A [`Language`] is compiled from a RON config. A [`Program`] is what the user
//! builds and saves. [`edit`] holds the connection rules. [`ast`] turns a
//! program into a tree for the consumer, with faults as [`ast::Problem`] nodes
//! in place. [`debug`] is the vocabulary between an editor and a runner.
//!
//! No geometry here: only stack positions are stored.

pub mod ast;
pub mod debug;
pub mod edit;
pub mod language;
pub mod literal;
pub mod program;
pub mod value;

pub use ast::{Ast, Expr, Node, Problem, Stmt};
pub use debug::{Annotation, DebugView, Pause, RunCommand, RunStatus, Runner};
pub use edit::{Fragment, Location, Target};
pub use language::{BlockDef, BlockKind, CategoryColor, Language};
pub use literal::{LiteralValidator, Validators};
pub use program::{Block, BlockId, Input, Program, Stack};
pub use value::Value;
