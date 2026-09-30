//! Checking what the user types into a slot.
//!
//! Literals are stored as typed and parsed only when the AST is built, so an
//! invalid one stays in the program to be fixed and is shown as a problem,
//! rather than being refused or quietly rewritten.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use crate::value::Value;

/// Consumer-supplied parsing for a `LiteralKind::Custom` type. Called on every
/// edit, so it should be cheap.
pub trait LiteralValidator: Debug + Send + Sync {
    /// `Err` is a short message for the tag under the slot.
    fn validate(&self, text: &str) -> Result<Value, String>;
}

/// Validators by the name `LiteralKind::Custom` refers to them by.
#[derive(Debug, Clone, Default)]
pub struct Validators(HashMap<String, Arc<dyn LiteralValidator>>);
