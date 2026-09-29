use serde::{Deserialize, Serialize};

/// A literal typed into a slot. Untagged so files read `10`, not `Number(10)`.
/// A dropdown choice is `Text`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    // Untagged tries variants in order; `true` must hit `Bool` first.
    Bool(bool),
    Number(f64),
    Text(String),
}

/// sRGB. Kept as a `"#rrggbb"` string in the config until compile, so a bad
/// color is one more `ConfigProblem` rather than a parse failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);
