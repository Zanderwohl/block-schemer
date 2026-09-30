//! The standalone editor.

use std::path::PathBuf;
use std::process::ExitCode;

use block_parse::language::Language;
use block_parse::program::Program;
use block_parse::Validators;
use block_parse_gui::{BlockEditor, EditorEvent};
use clap::Parser;
use eframe::egui;

#[derive(Parser)]
#[command(about = "Edit block programs for a block-parse language")]
struct Args {
    /// The language definition (RON, whatever its extension).
    #[arg(short, long)]
    language: Option<PathBuf>,
    /// The program to open. Created on first save if it does not exist;
    /// defaults to `untitled.<extension>`.
    program: Option<PathBuf>,
}

/// No interpreter: the window is for building and saving programs.
struct App {
    language: Language,
    program: Program,
    path: PathBuf,
    editor: BlockEditor,
    dirty: bool,
    status: String,
}

fn main() -> ExitCode {
    let args = Args::parse();
    // The only way to pick a language for now; others are planned.
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

    let path = args
        .program
        .unwrap_or_else(|| PathBuf::from(format!("untitled.{}", language.file.extension)));
    let mut status = String::new();
    let program = if path.exists() {
        match Program::load(&path, &language) {
            Ok((program, warnings)) => {
                status = warnings.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
                program
            }
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
    } else {
        Program::new(&language)
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("{} — block-parse", language.name))
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    let app = App {
        language,
        program,
        path,
        editor: BlockEditor::default(),
        dirty: false,
        status,
    };
    match eframe::run_native("block-parse-editor", options, Box::new(|_| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

impl App {
    fn save(&mut self) {
        self.status = match self.program.save(&self.path) {
            Ok(()) => {
                self.dirty = false;
                format!("saved {}", self.path.display())
            }
            Err(error) => format!("could not save {}: {error}", self.path.display()),
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(&self.language.name);
                ui.separator();
                let marker = if self.dirty { " •" } else { "" };
                ui.label(format!("{}{marker}", self.path.display()));
                if ui.button("Save").clicked() {
                    self.save();
                }
                ui.weak(&self.status);
            });
        });

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let output = self.editor.show(ui, &self.language, &mut self.program);
            if output.changed {
                self.dirty = true;
            }
            for event in output.events {
                if let EditorEvent::OpenDocumentation { link, .. } = event {
                    if link.starts_with("http://") || link.starts_with("https://") {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(link));
                    } else {
                        self.status = format!("documentation: {link}");
                    }
                }
            }
        });
    }
}
