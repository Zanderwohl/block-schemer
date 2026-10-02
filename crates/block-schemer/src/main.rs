//! `block-schemer [program.scmb]`, or with the `snapshot` feature,
//! `block-schemer --snapshot <program.scmb> <out.png> [--scale <n>]`.

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
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some("--snapshot".as_ref()) {
        return snapshot(args.collect());
    }
    let program = first.map(PathBuf::from);
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

#[cfg(feature = "snapshot")]
fn snapshot(args: Vec<std::ffi::OsString>) -> ExitCode {
    use block_parse_gui::{Theme, snapshot};

    let (paths, scale) = match &args[..] {
        [paths @ .., flag, scale] if flag == "--scale" => match scale.to_str().and_then(|s| s.parse().ok()) {
            Some(scale) => (paths, scale),
            None => {
                eprintln!("--scale takes a number of pixels per canvas unit");
                return ExitCode::from(2);
            }
        },
        paths => (paths, 2.0),
    };
    let [program, output] = paths else {
        eprintln!("usage: block-schemer --snapshot <program.scmb> <out.png> [--scale <n>]");
        return ExitCode::from(2);
    };
    let language = block_schemer::language();
    let image = snapshot::program_file(&language, program.as_ref(), &Theme::default(), scale);
    match image.and_then(|image| snapshot::save(&image, output.as_ref())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("could not snapshot: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "snapshot"))]
fn snapshot(_: Vec<std::ffi::OsString>) -> ExitCode {
    eprintln!("built without snapshots: rebuild with --features snapshot");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_decodes() {
        let icon = block_parse_gui::app::icon_from_png(super::ICON).unwrap();
        assert_eq!((icon.width, icon.height), (512, 512));
    }
}
