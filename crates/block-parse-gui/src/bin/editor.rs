//! The standalone editor.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use block_parse::language::Language;
use block_parse::Validators;
use block_parse_gui::app::{self, AppConfig, Menus};
use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(about = "Edit block programs for a block-parse language")]
struct Args {
    #[arg(short, long, value_enum, default_value_t = Command::Editor)]
    command: Command,
    /// The language definition (RON, whatever its extension).
    #[arg(short, long)]
    language: Option<PathBuf>,
    /// Where the File and Edit menus go.
    #[arg(long, value_enum, default_value_t)]
    menus: Menus,
    /// With `--command snapshot`: pixels per canvas unit.
    #[arg(long, default_value_t = 2.0)]
    scale: f32,
    /// The program to open, created on first save if it does not exist. For
    /// `snapshot`, the PNG to write.
    program: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Command {
    /// Open the editor window.
    Editor,
    /// Render every block in the language, a column per category, to a PNG.
    Snapshot,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let Some(language_path) = args.language else {
        eprintln!("no language given: pass --language <path>");
        return ExitCode::from(2);
    };
    let language = match Language::load(&language_path, &Validators::new()) {
        Ok(language) => language,
        Err(error) => {
            eprintln!("{}:\n{error}", language_path.display());
            return ExitCode::FAILURE;
        }
    };
    if args.command == Command::Snapshot {
        return snapshot(&language, args.program.as_deref(), args.scale);
    }
    app::run(AppConfig {
        name: "block-parse-editor".into(),
        language,
        program: args.program,
        menus: args.menus,
        runner: None,
        icon: None,
    })
}

#[cfg(feature = "snapshot")]
fn snapshot(language: &Language, output: Option<&Path>, scale: f32) -> ExitCode {
    let Some(output) = output else {
        eprintln!("no output given: pass the path of the PNG to write");
        return ExitCode::from(2);
    };
    let image = match block_parse_gui::snapshot::grid(language, &block_parse_gui::Theme::default(), scale) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("could not render: {error}");
            return ExitCode::FAILURE;
        }
    };
    match image.save_with_format(output, image::ImageFormat::Png) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", output.display());
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "snapshot"))]
fn snapshot(_: &Language, _: Option<&Path>, _: f32) -> ExitCode {
    eprintln!("built without snapshots: rebuild with --features snapshot");
    ExitCode::from(2)
}

