//! [`LanguageConfig`] is what serde reads, strings kept raw so reading fails
//! only on bad syntax. It compiles into [`Language`], which is fully checked;
//! nothing downstream re-validates.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::value::Value;

/// Always RON, whatever the file is named.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Language")]
pub struct LanguageConfig {
    pub name: String,
    pub file: FileConfig,
    /// `any` is built in.
    #[serde(default)]
    pub types: BTreeMap<String, TypeConfig>,
    /// Palette order.
    #[serde(default)]
    pub categories: Vec<CategoryConfig>,
    /// Palette order within categories.
    pub blocks: Vec<BlockConfig>,
}

/// The extension of this language's program files. Only a name for consumers
/// to bind file types to; the contents are RON regardless.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    /// Without the dot.
    pub extension: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeConfig {
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub literal: LiteralKind,
    /// Other types whose reporters fit this type's slots.
    #[serde(default)]
    pub accepts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryConfig {
    pub name: String,
    pub color: CategoryColor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockConfig {
    /// The opcode.
    pub id: String,
    /// For search results and listings; the block itself shows its spec.
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub kind: BlockKind,
    /// `{name:type=default}` is an input, `[name]` a branch, the rest label:
    /// `"if {cond:bool} then [then] else [else]"`.
    pub spec: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub documentation: Option<String>,
}

/// OKLCH. Only the hue is required; the GUI supplies the rest from its theme
/// and derives edges, shadows and highlights from it. Checked against these
/// ranges when compiled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CategoryColor {
    /// Degrees, `0.0..360.0`.
    pub hue: f32,
    /// `0.0..=0.37`. Many hues leave sRGB above about 0.15.
    #[serde(default)]
    pub chroma: Option<f32>,
    /// `0.0..=1.0`.
    #[serde(default)]
    pub lightness: Option<f32>,
}

/// Outline of a slot and of reporters producing the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Shape {
    #[default]
    Round,
    Hexagon,
    Square,
}

/// What may be typed into an empty slot.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum LiteralKind {
    /// A reporter must be plugged in; an empty slot is a `Problem`.
    #[default]
    None,
    Number,
    Text,
    Bool,
    Choice(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlockKind {
    /// Nothing above.
    Hat,
    #[default]
    Statement,
    /// Nothing below.
    Cap,
    /// Output type; lives in a slot, not a stack.
    Reporter(String),
}

#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    pub file: FileBinding,
    types: BTreeMap<String, TypeDef>,
    categories: Vec<Category>,
    blocks: Vec<BlockDef>,
    by_opcode: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBinding {
    pub extension: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub shape: Shape,
    pub literal: LiteralKind,
    pub accepts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub name: String,
    pub color: CategoryColor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockDef {
    pub opcode: String,
    pub name: String,
    pub kind: BlockKind,
    /// Uncategorized blocks are drawn in the GUI's neutral swatch.
    pub category: Option<usize>,
    /// The parsed spec, in reading order.
    pub parts: Vec<Part>,
    /// Finer than a category (`"string manipulation"`), for consumers to
    /// search and sort by. No effect on drawing or rules.
    pub tags: Vec<String>,
    /// One paragraph, for a tooltip.
    pub description: Option<String>,
    /// A URL or a relative path; resolving it is the consumer's business.
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Label(String),
    Input(InputDef),
    /// Starts a new row after it.
    Branch(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputDef {
    pub name: String,
    pub ty: String,
    /// Checked against the type's literal kind. `None` uses the type's default.
    pub default: Option<Value>,
}

#[derive(Debug)]
pub enum LanguageError {
    Syntax(ron::error::SpannedError),
    Io(std::io::Error),
    /// Every problem found, not just the first.
    Invalid(Vec<ConfigProblem>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigProblem {
    /// Opcode of the block at fault, if any.
    pub block: Option<String>,
    pub message: String,
}
