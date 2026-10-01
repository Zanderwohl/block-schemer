//! Language definitions, programs and ASTs for block-based editors.
//!
//! A [`Language`] is compiled from a RON config. A [`Program`] is what the user
//! builds and saves. [`edit`] holds the connection rules. [`ast`] turns a
//! program into a tree for the consumer, with faults as [`ast::Problem`] nodes
//! in place. [`host`] is the vocabulary between an editor and its host.
//!
//! No geometry here: only stack positions are stored.

pub mod ast;
mod build;
pub mod edit;
pub mod host;
pub mod language;
pub mod literal;
pub mod program;
mod spec;
pub mod value;

pub use ast::{Ast, Expr, Node, Problem, Stmt};
pub use host::{Annotation, Highlight, HighlightStyle, Overlay, RunCommand, RunStatus, Runner};
pub use edit::{Fragment, Location, Target};
pub use language::{BlockDef, BlockKind, CategoryColor, Fit, Language};
pub use literal::{LiteralValidator, Validators};
pub use program::{Block, BlockId, Input, Program, Slot, Stack};
pub use value::Value;
