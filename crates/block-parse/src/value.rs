use serde::{Deserialize, Serialize};

/// A parsed literal. Which variant comes out depends on the slot type's
/// literal kind; arithmetic and promotion between them are the back end's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Float(f64),
    /// Minor units: `12.34` is `Currency(1234)`.
    Currency(i64),
    /// Includes dropdown choices.
    Text(String),
}
