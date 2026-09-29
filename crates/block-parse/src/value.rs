use serde::{Deserialize, Serialize};

/// A parsed literal. Untagged so serialized ASTs read `10`, not `Number(10)`.
/// A dropdown choice is `Text`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    // Untagged tries variants in order; `true` must hit `Bool` first.
    Bool(bool),
    Number(f64),
    Text(String),
}
