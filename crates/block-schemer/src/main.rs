//! `block-schemer [program.scmb]`

use std::path::PathBuf;
use std::process::ExitCode;

use block_parse_gui::app::{self, AppConfig};
use block_schemer::{SchemerRunner, Steel};

const ICON: &[u8] = include_bytes!("../assets/icons/icon-512.png");

fn main() -> ExitCode {
    let program = std::env::args_os().nth(1).map(PathBuf::from);
    let language = block_schemer::language();
    app::run(AppConfig {
        name: "Block Schemer".into(),
        runner: Some(Box::new(SchemerRunner::new(language.clone(), Steel::new()))),
        language,
        program,
        menu_bar: true,
        icon: Some(app::icon_from_png(ICON).expect("the built-in icon is a PNG")),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_decodes() {
        let icon = block_parse_gui::app::icon_from_png(super::ICON).unwrap();
        assert_eq!((icon.width, icon.height), (512, 512));
    }
}
