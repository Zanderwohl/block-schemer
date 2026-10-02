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
    /// With `--command snapshot`: only blocks with one of these tags, as
    /// `--tags="foo,bar"`. Every block when left out.
    #[arg(long, value_delimiter = ',')]
    tags: Option<Vec<String>>,
    /// The program to open, created on first save if it does not exist. For
    /// `snapshot`, the program to render, or with no PNG after it, the PNG
    /// to write the language's blocks to.
    program: Option<PathBuf>,
    /// With `--command snapshot`: the PNG to write the program to.
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Command {
    /// Open the editor window.
    Editor,
    /// Render a program to a PNG, or without one, every block in the
    /// language, a column per category.
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
    if args.tags.is_some() && args.command != Command::Snapshot {
        eprintln!("--tags is for --command snapshot");
        return ExitCode::from(2);
    }
    if args.output.is_some() && args.command != Command::Snapshot {
        eprintln!("only --command snapshot takes a second path");
        return ExitCode::from(2);
    }
    if args.command == Command::Snapshot {
        let (program, output) = match (args.program, args.output) {
            (Some(program), Some(output)) => (Some(program), output),
            (Some(output), None) => (None, output),
            (None, _) => {
                eprintln!("no output given: pass the path of the PNG to write");
                return ExitCode::from(2);
            }
        };
        if program.is_some() && args.tags.is_some() {
            eprintln!("--tags is for snapshots of the language's blocks, not of a program");
            return ExitCode::from(2);
        }
        let unknown: Vec<&str> = args
            .tags
            .iter()
            .flatten()
            .filter(|tag| !language.tags().contains(tag))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            eprintln!("no block is tagged {}", unknown.join(", "));
            return ExitCode::from(2);
        }
        return snapshot(&language, program.as_deref(), args.tags.as_deref(), &output, args.scale);
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
fn snapshot(language: &Language, program: Option<&Path>, tags: Option<&[String]>, output: &Path, scale: f32) -> ExitCode {
    use block_parse_gui::{Theme, snapshot};

    let image = match program {
        Some(program) => snapshot::program_file(language, program, &Theme::default(), scale),
        None => snapshot::grid(language, tags, &Theme::default(), scale),
    };
    match image.and_then(|image| snapshot::save(&image, output)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("could not snapshot: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "snapshot"))]
fn snapshot(_: &Language, _: Option<&Path>, _: Option<&[String]>, _: &Path, _: f32) -> ExitCode {
    eprintln!("built without snapshots: rebuild with --features snapshot");
    ExitCode::from(2)
}
