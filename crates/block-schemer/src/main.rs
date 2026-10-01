//! `block-schemer [program.scmb]`

// Without it, Windows opens a console window beside the app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use block_parse_gui::app::{self, AppConfig, Menus};
use block_schemer::{Native, SchemerRunner, Steel};

// eframe sets the Dock icon from this, over the app bundle's, so the Mac one
// needs its rounded shape baked in.
#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../assets/icons/block-schemer.iconset/icon_512x512.png");
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../assets/icons/icon-512.png");

fn main() -> ExitCode {
    let program = std::env::args_os().nth(1).map(PathBuf::from);
    let language = block_schemer::language();
    app::run(AppConfig {
        name: "Block Schemer".into(),
        runner: Some(Box::new(SchemerRunner::new(language.clone(), Native::spawn(Steel::new)))),
        language,
        program,
        menus: Menus::Native,
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
