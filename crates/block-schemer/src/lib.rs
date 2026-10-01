//! Block Schemer: Scheme programs built from blocks.
//!
//! The editor builds an AST; [`codegen`] turns it into Scheme text, refusing
//! anything the language does not offer, and a [`Scheme`] runs it. Steel runs
//! it for now; another interpreter only needs to implement [`Scheme`].

pub mod codegen;
pub mod literals;
pub mod runner;
pub mod scheme;

use block_parse::{Language, Validators};

pub use runner::SchemerRunner;
pub use scheme::{Scheme, Steel};

const LANGUAGE: &str = include_str!("../scheme.ron");

/// The validators the language's `Custom` types name.
pub fn validators() -> Validators {
    let mut validators = Validators::new();
    validators.insert("datum", std::sync::Arc::new(literals::Datum));
    validators.insert("symbol", std::sync::Arc::new(literals::Symbol));
    validators
}

pub fn language() -> Language {
    Language::from_ron(LANGUAGE, &validators()).expect("the built-in language compiles")
}
