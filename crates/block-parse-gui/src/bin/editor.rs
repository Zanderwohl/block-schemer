//! `block-parse-editor <language> [program]`

use std::path::PathBuf;

use block_parse::language::Language;
use block_parse::program::Program;
use block_parse_gui::BlockEditor;

/// No interpreter: run controls hidden, AST panel shows what a consumer gets.
#[allow(dead_code)]
struct App {
    language: Language,
    program: Program,
    /// `None` until the first Save As.
    path: Option<PathBuf>,
    editor: BlockEditor,
    show_ast: bool,
    dirty: bool,
}

fn main() {
    todo!("types only for now")
}
