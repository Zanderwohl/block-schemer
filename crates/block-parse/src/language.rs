//! [`LanguageConfig`] is what serde reads, strings kept raw so reading fails
//! only on bad syntax. It compiles into [`Language`], which is fully checked;
//! nothing downstream re-validates.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::literal::{self, Validators};
use crate::spec::{self, Arity, SpecPart};
use crate::value::Value;

/// Always RON, whatever the file is named.
///
/// Opcodes, type names, input names and branch names are non-empty printable
/// ASCII without spaces (`!` to `~`), so none can hide control or
/// bidirectional characters. Type, input and branch names also exclude
/// `{ } [ ] : = * +`, which delimit them in specs. Labels and descriptions are
/// free text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Language")]
pub struct LanguageConfig {
    pub name: String,
    pub file: FileConfig,
    /// No types are built in.
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeConfig {
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub literal: LiteralKind,
    /// Reporters this type's slots take besides its own.
    #[serde(default)]
    pub accepts: TypeSet,
    /// Slots this type's reporters go into besides its own. `All` is for
    /// values typed only at run time, such as a variable getter's.
    #[serde(default)]
    pub fits: TypeSet,
}

/// One side of type compatibility. A reporter fits a slot when the types are
/// equal, the slot `accepts` it or the reporter `fits` the slot; the last two
/// put an `Expr::Convert` in the AST so the consumer decides what that means.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TypeSet {
    #[default]
    Exactly,
    Types(Vec<String>),
    All,
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
    /// `{name:type=default}` is an input, `{name:type*}` a list of any length,
    /// `{name:type+}` one of at least one item, `[name]` a branch, the rest
    /// label: `"if {cond:bool} then [then] else [else]"`. A word wrapped in
    /// underscores, as `_then_`, is a faint label: a reading aid that is not
    /// part of the language.
    pub spec: String,
    #[serde(default)]
    pub layout: BlockLayout,
    /// Shown in a blank slot, by input or list name; the name itself
    /// otherwise. Free text.
    #[serde(default)]
    pub hints: BTreeMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub documentation: Option<String>,
    /// A checkbox whose state the host owns and supplies in `Overlay`, never
    /// saved in the program. Stack blocks only.
    #[serde(default)]
    pub switch: bool,
    /// Replaces the category's color for this block alone; the block stays
    /// under its category in the palette.
    #[serde(default)]
    pub color: Option<CategoryColor>,
}

const COLOR_RANGE: &str = "hue 0..360, chroma 0..=0.37, lightness 0..=1";

/// OKLCH. Only the hue is required; the GUI supplies the rest from its theme.
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

impl CategoryColor {
    fn in_range(&self) -> bool {
        let within = |value: f32, range: std::ops::RangeInclusive<f32>| value.is_finite() && range.contains(&value);
        within(self.hue, 0.0..=360.0)
            && self.hue < 360.0
            && self.chroma.is_none_or(|c| within(c, 0.0..=0.37))
            && self.lightness.is_none_or(|l| within(l, 0.0..=1.0))
    }
}

/// Outline of a slot and of reporters producing the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Shape {
    #[default]
    Round,
    Hexagon,
    Square,
}

/// What may be typed into an empty slot, and how it is checked. See
/// [`literal`](crate::literal).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum LiteralKind {
    /// A reporter must be plugged in; an empty slot is a `Problem`.
    #[default]
    None,
    /// `Value::Float`, e-notation allowed. Must be finite: `inf`, `NaN` and
    /// `1e999` are invalid.
    Float,
    /// `Value::Integer`.
    Integer,
    /// `Value::Integer` if written without `.` or `e` and it fits an i64,
    /// otherwise as `Float`, so the back end chooses promotion.
    Number,
    /// `Value::Currency` in minor units. No decimal places or exactly two:
    /// `12` and `12.30` are 1230; `12.3` is invalid.
    Currency,
    /// `Value::Unsigned`. `101` or `0b101`.
    Binary,
    /// `Value::Unsigned`. `ff`, `0xff` or `#ff`, either case.
    Hex,
    Text,
    /// A checkbox, stored as `"true"` or `"false"`; `"false"` when the spec
    /// gives no default.
    Bool,
    Choice(Vec<String>),
    /// A validator the consumer registers under this name before compiling.
    Custom(String),
}

/// Where a block's rows break. Per block rather than per use, so a program
/// always reads the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlockLayout {
    /// One row, before any branch.
    #[default]
    Inline,
    /// The first `n` inputs, a list counting as one, share the first row;
    /// each later input and list item gets an indented row. Blocks without
    /// branches only. See `documentation/03-variadic.md`.
    Body(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlockKind {
    /// Nothing above.
    Hat,
    #[default]
    Statement,
    /// Nothing below.
    Cap,
    /// Nothing above or below: a whole script in one block.
    HatCap,
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
    /// Every `Custom` literal is resolved here, or the language fails to compile.
    validators: Validators,
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
    pub accepts: TypeSet,
    pub fits: TypeSet,
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
    pub switch: bool,
    /// Overrides the category's color.
    pub color: Option<CategoryColor>,
    pub layout: BlockLayout,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Label(Label),
    Input(InputDef),
    List(ListDef),
    /// Starts a new row after it.
    Branch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub text: String,
    /// Drawn smaller and fainter.
    pub faint: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputDef {
    pub name: String,
    pub ty: String,
    /// Source text, already validated: the spec's default, else the literal
    /// kind's blank. `None` only for types that take no literal.
    pub default: Option<String>,
    /// For a blank slot.
    pub hint: String,
}

/// Any number of inputs of one type under one name. Starts empty: a list
/// takes no default.
#[derive(Debug, Clone, PartialEq)]
pub struct ListDef {
    pub name: String,
    pub ty: String,
    /// 0 for `*`, 1 for `+`.
    pub min: usize,
    /// For each blank item and the empty slot.
    pub hint: String,
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

/// How a reporter's output relates to a slot's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Exact,
    /// Allowed by `accepts` or `fits`; becomes `Expr::Convert`.
    Convert,
    No,
}

impl TypeSet {
    fn includes(&self, name: &str) -> bool {
        match self {
            Self::Exactly => false,
            Self::Types(names) => names.iter().any(|n| n == name),
            Self::All => true,
        }
    }

    fn names(&self) -> &[String] {
        match self {
            Self::Types(names) => names,
            Self::Exactly | Self::All => &[],
        }
    }
}

impl BlockKind {
    pub fn output(&self) -> Option<&str> {
        match self {
            Self::Reporter(ty) => Some(ty),
            Self::Hat | Self::Statement | Self::Cap | Self::HatCap => None,
        }
    }

    /// Nothing goes above it.
    pub fn is_hat(&self) -> bool {
        matches!(self, Self::Hat | Self::HatCap)
    }

    /// Nothing goes below it.
    pub fn is_cap(&self) -> bool {
        matches!(self, Self::Cap | Self::HatCap)
    }
}

impl BlockDef {
    /// Single inputs only; see [`lists`](Self::lists).
    pub fn inputs(&self) -> impl Iterator<Item = &InputDef> {
        self.parts.iter().filter_map(|part| match part {
            Part::Input(input) => Some(input),
            Part::Label(_) | Part::List(_) | Part::Branch(_) => None,
        })
    }

    pub fn lists(&self) -> impl Iterator<Item = &ListDef> {
        self.parts.iter().filter_map(|part| match part {
            Part::List(list) => Some(list),
            Part::Label(_) | Part::Input(_) | Part::Branch(_) => None,
        })
    }

    pub fn list(&self, name: &str) -> Option<&ListDef> {
        self.lists().find(|list| list.name == name)
    }

    /// The type a slot of this block takes, whatever the item index.
    pub fn slot_type(&self, slot: &crate::program::Slot) -> Option<&str> {
        match slot.item {
            None => self.input(&slot.input).map(|input| input.ty.as_str()),
            Some(_) => self.list(&slot.input).map(|list| list.ty.as_str()),
        }
    }

    pub fn input(&self, name: &str) -> Option<&InputDef> {
        self.inputs().find(|input| input.name == name)
    }

    pub fn branches(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            Part::Branch(name) => Some(name.as_str()),
            Part::Label(_) | Part::Input(_) | Part::List(_) => None,
        })
    }

    pub fn has_branch(&self, name: &str) -> bool {
        self.branches().any(|branch| branch == name)
    }
}

impl Language {
    pub fn from_ron(text: &str, validators: &Validators) -> Result<Self, LanguageError> {
        let config: LanguageConfig = ron_options()
            .from_str(text)
            .map_err(LanguageError::Syntax)?;
        config.compile(validators)
    }

    pub fn load(path: impl AsRef<Path>, validators: &Validators) -> Result<Self, LanguageError> {
        let text = std::fs::read_to_string(path).map_err(LanguageError::Io)?;
        Self::from_ron(&text, validators)
    }

    pub fn ty(&self, name: &str) -> Option<&TypeDef> {
        self.types.get(name)
    }

    pub fn types(&self) -> impl Iterator<Item = &TypeDef> {
        self.types.values()
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    /// Palette order.
    pub fn blocks(&self) -> &[BlockDef] {
        &self.blocks
    }

    pub fn block(&self, opcode: &str) -> Option<&BlockDef> {
        self.by_opcode.get(opcode).map(|&index| &self.blocks[index])
    }

    pub fn validators(&self) -> &Validators {
        &self.validators
    }

    pub fn fit(&self, output: &str, slot: &str) -> Fit {
        if output == slot {
            return Fit::Exact;
        }
        let accepts = self.ty(slot).is_some_and(|ty| ty.accepts.includes(output));
        let fits = self.ty(output).is_some_and(|ty| ty.fits.includes(slot));
        if accepts || fits { Fit::Convert } else { Fit::No }
    }

    pub fn parse_literal(&self, ty: &str, text: &str) -> Result<Value, String> {
        let ty = self.ty(ty).ok_or_else(|| format!("unknown type `{ty}`"))?;
        literal::parse(&ty.literal, text, &self.validators)
    }

    /// What a slot of type `ty` keeps once the user leaves it.
    pub fn normalize_literal(&self, ty: &str, text: &str) -> String {
        match self.ty(ty) {
            Some(ty) => literal::normalize(&ty.literal, text, &self.validators),
            None => text.to_owned(),
        }
    }
}

/// RON as language and program files are read: `Some` may be left implicit.
pub(crate) fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

fn is_name(name: &str, in_spec: bool) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && !(in_spec && b"{}[]:=*+".contains(&b)))
}

impl LanguageConfig {
    pub fn compile(self, validators: &Validators) -> Result<Language, LanguageError> {
        let mut problems = Vec::new();
        let mut problem = |block: Option<&str>, message: String| {
            problems.push(ConfigProblem {
                block: block.map(Into::into),
                message,
            })
        };

        if self.name.trim().is_empty() {
            problem(None, "the language needs a name".into());
        }
        if !is_name(&self.file.extension, false) || self.file.extension.starts_with('.') {
            problem(
                None,
                format!(
                    "file extension `{}` must be printable ASCII without spaces or a leading dot",
                    self.file.extension
                ),
            );
        }

        let mut types = BTreeMap::new();
        for (name, config) in &self.types {
            if !is_name(name, true) {
                problem(None, format!("type name `{name}` is not a valid name"));
            }
            for other in config.accepts.names().iter().chain(config.fits.names()) {
                if !self.types.contains_key(other) {
                    problem(None, format!("type `{name}` refers to unknown type `{other}`"));
                }
            }
            match &config.literal {
                LiteralKind::Custom(validator) if validators.get(validator).is_none() => problem(
                    None,
                    format!("type `{name}` uses validator `{validator}`, which is not registered"),
                ),
                LiteralKind::Choice(options) if options.is_empty() => {
                    problem(None, format!("type `{name}` offers no choices"))
                }
                _ => {}
            }
            types.insert(
                name.clone(),
                TypeDef {
                    name: name.clone(),
                    shape: config.shape,
                    literal: config.literal.clone(),
                    accepts: config.accepts.clone(),
                    fits: config.fits.clone(),
                },
            );
        }

        let mut categories = Vec::new();
        let mut category_index = HashMap::new();
        for config in &self.categories {
            if category_index
                .insert(config.name.clone(), categories.len())
                .is_some()
            {
                problem(None, format!("category `{}` is declared twice", config.name));
            }
            if !config.color.in_range() {
                problem(
                    None,
                    format!("category `{}` color is out of range ({COLOR_RANGE})", config.name),
                );
            }
            categories.push(Category {
                name: config.name.clone(),
                color: config.color,
            });
        }

        let parse_default = |ty: &TypeDef, text: &str| literal::parse(&ty.literal, text, validators);

        let mut blocks = Vec::new();
        let mut by_opcode = HashMap::new();
        for config in self.blocks {
            let opcode = config.id.as_str();
            let at = Some(opcode);
            if !is_name(opcode, false) {
                problem(at, format!("opcode `{opcode}` is not a valid name"));
            }
            if by_opcode.insert(config.id.clone(), blocks.len()).is_some() {
                problem(at, format!("opcode `{opcode}` is declared twice"));
            }
            if config.name.trim().is_empty() {
                problem(at, "the block needs a name".into());
            }
            if config.color.is_some_and(|color| !color.in_range()) {
                problem(at, format!("color is out of range ({COLOR_RANGE})"));
            }
            let category = match &config.category {
                Some(name) => {
                    let index = category_index.get(name).copied();
                    if index.is_none() {
                        problem(at, format!("unknown category `{name}`"));
                    }
                    index
                }
                None => None,
            };
            if let Some(output) = config.kind.output()
                && !types.contains_key(output)
            {
                problem(at, format!("reports unknown type `{output}`"));
            }

            if config.switch && config.kind.output().is_some() {
                problem(at, "switches go on stack blocks, not reporters".into());
            }

            let spec_parts = match spec::parse(&config.spec) {
                Ok(parts) => parts,
                Err(message) => {
                    problem(at, format!("spec: {message}"));
                    Vec::new()
                }
            };
            if spec_parts.is_empty() {
                problem(at, "spec is empty".into());
            }
            if matches!(config.layout, BlockLayout::Body(_))
                && spec_parts.iter().any(|part| matches!(part, SpecPart::Branch(_)))
            {
                problem(at, "a block with branches already has rows; it cannot take `Body`".into());
            }

            let mut names = HashSet::new();
            let mut parts = Vec::new();
            for part in spec_parts {
                match part {
                    SpecPart::Label(label) => parts.push(Part::Label(label)),
                    SpecPart::Branch(name) => {
                        if !is_name(&name, true) {
                            problem(at, format!("branch name `{name}` is not a valid name"));
                        }
                        if !names.insert(name.clone()) {
                            problem(at, format!("`{name}` is used twice in the spec"));
                        }
                        if config.kind.is_hat() || config.kind.output().is_some() {
                            problem(at, "hats and reporters cannot have branches".into());
                        }
                        parts.push(Part::Branch(name));
                    }
                    SpecPart::Input { name, ty, default, list } => {
                        if !is_name(&name, true) {
                            problem(at, format!("input name `{name}` is not a valid name"));
                        }
                        if !names.insert(name.clone()) {
                            problem(at, format!("`{name}` is used twice in the spec"));
                        }
                        if let Some(arity) = list {
                            if !types.contains_key(&ty) {
                                problem(at, format!("list `{name}` has unknown type `{ty}`"));
                            }
                            if default.is_some() {
                                problem(at, format!("list `{name}` has a default, but lists start empty"));
                            }
                            let min = match arity {
                                Arity::Any => 0,
                                Arity::AtLeastOne => 1,
                            };
                            let hint = config.hints.get(&name).cloned().unwrap_or_else(|| name.clone());
                            parts.push(Part::List(ListDef { name, ty, min, hint }));
                            continue;
                        }
                        let default = match types.get(&ty) {
                            None => {
                                problem(at, format!("input `{name}` has unknown type `{ty}`"));
                                None
                            }
                            Some(def) => match (&def.literal, default) {
                                (LiteralKind::None, Some(_)) => {
                                    problem(
                                        at,
                                        format!("input `{name}` has a default, but `{ty}` takes no typed value"),
                                    );
                                    None
                                }
                                (kind, None) => literal::blank(kind),
                                (_, Some(text)) => {
                                    if let Err(message) = parse_default(def, &text) {
                                        problem(at, format!("default for `{name}`: {message}"));
                                    }
                                    Some(text)
                                }
                            },
                        };
                        let hint = config.hints.get(&name).cloned().unwrap_or_else(|| name.clone());
                        parts.push(Part::Input(InputDef { name, ty, default, hint }));
                    }
                }
            }

            for name in config.hints.keys() {
                if !parts.iter().any(|part| match part {
                    Part::Input(input) => &input.name == name,
                    Part::List(list) => &list.name == name,
                    Part::Label(_) | Part::Branch(_) => false,
                }) {
                    problem(at, format!("hint for `{name}`, which is no input or list"));
                }
            }

            blocks.push(BlockDef {
                opcode: config.id,
                name: config.name,
                kind: config.kind,
                category,
                parts,
                tags: config.tags,
                description: config.description,
                documentation: config.documentation,
                switch: config.switch,
                color: config.color,
                layout: config.layout,
            });
        }

        if !problems.is_empty() {
            return Err(LanguageError::Invalid(problems));
        }
        Ok(Language {
            name: self.name,
            file: FileBinding {
                description: self
                    .file
                    .description
                    .unwrap_or_else(|| format!("{} program", self.file.extension)),
                extension: self.file.extension,
            },
            types,
            categories,
            blocks,
            by_opcode,
            validators: validators.clone(),
        })
    }
}

impl std::fmt::Display for LanguageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Invalid(problems) => {
                for (index, problem) in problems.iter().enumerate() {
                    if index > 0 {
                        writeln!(f)?;
                    }
                    match &problem.block {
                        Some(block) => write!(f, "block `{block}`: {}", problem.message)?,
                        None => write!(f, "{}", problem.message)?,
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LanguageError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(text: &str) -> Result<Language, Vec<String>> {
        Language::from_ron(text, &Validators::new()).map_err(|error| match error {
            LanguageError::Invalid(problems) => problems.into_iter().map(|p| p.message).collect(),
            other => vec![other.to_string()],
        })
    }

    #[test]
    fn every_example_language_compiles() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/languages");
        let mut seen = 0;
        for entry in std::fs::read_dir(dir).expect("the examples directory") {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "ron") {
                if let Err(error) = Language::load(&path, &Validators::new()) {
                    panic!("{}:\n{error}", path.display());
                }
                seen += 1;
            }
        }
        assert!(seen >= 2, "found only {seen} example languages");
    }

    const MINIMAL: &str = r#"Language(
        name: "t",
        file: (extension: "t"),
        types: { "number": (literal: Float), "bool": (shape: Hexagon, literal: Bool) },
        categories: [(name: "C", color: (hue: 10.0))],
        blocks: [
            (id: "go", name: "Go", category: "C", kind: Hat, spec: "go"),
            (id: "if", name: "If", spec: "if {c:bool} [then]"),
            (id: "add", name: "Add", kind: Reporter("number"), spec: "{a:number=1} + {b:number}"),
        ],
    )"#;

    #[test]
    fn defaults_fall_back_to_the_literal_kinds_blank() {
        let language = compile(MINIMAL).unwrap();
        let add = language.block("add").unwrap();
        assert_eq!(add.input("a").unwrap().default.as_deref(), Some("1"));
        assert_eq!(add.input("b").unwrap().default.as_deref(), Some(""));
        let branch = language.block("if").unwrap();
        assert_eq!(branch.input("c").unwrap().default.as_deref(), Some("false"));
        assert!(branch.has_branch("then"));
    }

    #[test]
    fn every_problem_is_reported_not_just_the_first() {
        let text = MINIMAL
            .replace(r#"category: "C", kind: Hat"#, r#"category: "Nope", kind: Hat"#)
            .replace("{a:number=1}", "{a:number=one}")
            .replace(r#"id: "if""#, r#"id: "i f""#)
            .replace("hue: 10.0", "hue: 400.0");
        let problems = compile(&text).unwrap_err();
        assert_eq!(problems.len(), 4, "{problems:#?}");
    }

    #[test]
    fn a_block_may_override_its_categorys_color_within_range() {
        let language = compile(&MINIMAL.replace(
            r#"category: "C", kind: Hat, spec: "go""#,
            r#"category: "C", kind: Hat, spec: "go", color: Some((hue: 200.0))"#,
        ))
        .unwrap();
        let go = language.block("go").unwrap();
        assert_eq!(go.color.map(|color| color.hue), Some(200.0));
        assert_eq!(go.category, Some(0), "it stays in its category");
        assert!(language.block("if").unwrap().color.is_none(), "none unless asked for");

        let problems = compile(&MINIMAL.replace(
            r#"category: "C", kind: Hat, spec: "go""#,
            r#"category: "C", kind: Hat, spec: "go", color: Some((hue: 400.0))"#,
        ))
        .unwrap_err();
        assert!(problems[0].contains("out of range"), "{problems:#?}");
    }

    #[test]
    fn only_stack_blocks_take_a_switch() {
        let language = compile(&MINIMAL.replace(r#"spec: "if {c:bool} [then]""#, r#"spec: "if {c:bool} [then]", switch: true"#))
            .unwrap();
        assert!(language.block("if").unwrap().switch);
        assert!(!language.block("go").unwrap().switch, "off unless asked for");

        let problems = compile(&MINIMAL.replace(
            r#"spec: "{a:number=1} + {b:number}""#,
            r#"spec: "{a:number=1} + {b:number}", switch: true"#,
        ))
        .unwrap_err();
        assert!(problems[0].contains("switch"), "{problems:#?}");
    }

    #[test]
    fn names_in_specs_may_not_hold_delimiters() {
        assert!(is_name("a:b.c,d-e", false));
        assert!(!is_name("a:b", true));
        assert!(!is_name("a b", false));
        assert!(!is_name("a\u{7}", false));
        assert!(!is_name("", false));
        assert!(!is_name("a*", true) && !is_name("a+", true));
        let problems = compile(&MINIMAL.replace("{a:number=1}", "{a:b:c}")).unwrap_err();
        assert!(problems.iter().any(|p| p.contains("b:c")), "{problems:#?}");
    }

    #[test]
    fn lists_start_empty_and_know_their_minimum() {
        let language = compile(&MINIMAL.replace(
            r#"spec: "{a:number=1} + {b:number}""#,
            r#"spec: "sum {a:number=1} {rest:number*} {more:number+}", layout: Body(1)"#,
        ))
        .unwrap();
        let add = language.block("add").unwrap();
        assert_eq!(add.inputs().map(|input| input.name.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!(add.list("rest").map(|list| list.min), Some(0));
        assert_eq!(add.list("more").map(|list| list.min), Some(1));
        assert_eq!(add.layout, BlockLayout::Body(1));
        assert_eq!(language.block("if").unwrap().layout, BlockLayout::Inline);
    }

    #[test]
    fn hints_default_to_the_input_name() {
        let language = compile(&MINIMAL.replace(
            r#"spec: "{a:number=1} + {b:number}""#,
            r#"spec: "{a:number=1} + {b:number} {rest:number*}", hints: {"b": "a number", "rest": "operand"}"#,
        ))
        .unwrap();
        let add = language.block("add").unwrap();
        assert_eq!(add.input("a").unwrap().hint, "a");
        assert_eq!(add.input("b").unwrap().hint, "a number");
        assert_eq!(add.list("rest").unwrap().hint, "operand");

        let problems = compile(&MINIMAL.replace(
            r#"spec: "{a:number=1} + {b:number}""#,
            r#"spec: "{a:number=1} + {b:number}", hints: {"c": "nothing"}"#,
        ))
        .unwrap_err();
        assert!(problems[0].contains("`c`"), "{problems:#?}");
    }

    #[test]
    fn lists_take_no_default_and_body_takes_no_branches() {
        let problems = compile(
            &MINIMAL
                .replace("{b:number}", "{b:number*=3}")
                .replace(r#"spec: "if {c:bool} [then]""#, r#"spec: "if {c:bool} [then]", layout: Body(1)"#),
        )
        .unwrap_err();
        assert_eq!(problems.len(), 2, "{problems:#?}");
        assert!(problems.iter().any(|p| p.contains("lists start empty")), "{problems:#?}");
        assert!(problems.iter().any(|p| p.contains("Body")), "{problems:#?}");
    }

    #[test]
    fn accepts_and_fits_decide_conversions() {
        let text = MINIMAL.replace(
            r#""number": (literal: Float)"#,
            r#""number": (literal: Float), "text": (literal: Text, accepts: All), "value": (fits: Types(["bool"]))"#,
        );
        let language = compile(&text).unwrap();
        assert_eq!(language.fit("number", "number"), Fit::Exact);
        assert_eq!(language.fit("number", "text"), Fit::Convert);
        assert_eq!(language.fit("text", "number"), Fit::No);
        assert_eq!(language.fit("value", "bool"), Fit::Convert);
        assert_eq!(language.fit("value", "number"), Fit::No);
    }
}
