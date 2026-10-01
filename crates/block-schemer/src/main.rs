//! `block-schemer [program.scmb]`

use std::path::PathBuf;
use std::process::ExitCode;

use block_parse_gui::app::{self, AppConfig};
use block_schemer::{SchemerRunner, Steel};

fn main() -> ExitCode {
    let program = std::env::args_os().nth(1).map(PathBuf::from);
    let language = block_schemer::language();
    app::run(AppConfig {
        name: "Block Schemer".into(),
        runner: Some(Box::new(SchemerRunner::new(language.clone(), Steel::new()))),
        language,
        program,
        menu_bar: true,
    })
}
