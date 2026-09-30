//! The standalone editor.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use block_parse::language::Language;
use block_parse::program::Program;
use block_parse::Validators;
use block_parse_gui::{BlockEditor, EditorEvent};
use clap::Parser;
use eframe::egui::{self, Button, Key, KeyboardShortcut, Modifiers, ViewportCommand};

#[derive(Parser)]
#[command(about = "Edit block programs for a block-parse language")]
struct Args {
    /// The language definition (RON, whatever its extension).
    #[arg(short, long)]
    language: Option<PathBuf>,
    /// Hide the egui menu bar, for hosts that provide native menus.
    #[arg(long)]
    no_menu_bar: bool,
    /// The program to open. Created on first save if it does not exist.
    program: Option<PathBuf>,
}

const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut = KeyboardShortcut::new(COMMAND_SHIFT, Key::S);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(COMMAND_SHIFT, Key::Z);
const COMMAND_SHIFT: Modifiers = Modifiers {
    shift: true,
    command: true,
    ..Modifiers::NONE
};

/// Something that would lose unsaved changes, waiting on the save prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    New,
    Open,
    Quit,
}

enum Choice {
    Save,
    Discard,
    Cancel,
}

/// No interpreter: the window is for building and saving programs.
struct App {
    language: Language,
    program: Program,
    /// `None` until first saved or opened.
    path: Option<PathBuf>,
    editor: BlockEditor,
    dirty: bool,
    status: String,
    menu_bar: bool,
    pending: Option<Pending>,
    /// Set once quitting is agreed, so the close request is let through.
    closing: bool,
    title: String,
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

    let mut status = String::new();
    let program = match &args.program {
        Some(path) if path.exists() => match Program::load(path, &language) {
            Ok((program, warnings)) => {
                status = join(&warnings);
                program
            }
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        },
        _ => Program::new(&language),
    };

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        // Its Quit item terminates the process without a close request, so
        // the save prompt would never get the chance to run.
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_default_menu(false);
        }));
    }

    let app = App {
        language,
        program,
        path: args.program,
        editor: BlockEditor::default(),
        dirty: false,
        status,
        menu_bar: !args.no_menu_bar,
        pending: None,
        closing: false,
        title: String::new(),
    };
    match eframe::run_native("block-parse-editor", options, Box::new(|_| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        // A run in hand is out of the program, so saving now would drop it.
        if self.pending.is_none() && !self.editor.is_dragging() {
            self.shortcuts(&ctx);
        }

        if self.menu_bar {
            egui::Panel::top("menu_bar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| self.menus(ui));
            });
        }
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(&self.language.name);
                ui.separator();
                let marker = if self.dirty { " •" } else { "" };
                ui.label(format!("{}{marker}", self.display_name()));
                ui.separator();
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
                        ctx.open_url(egui::OpenUrl::new_tab(link));
                    } else {
                        self.status = format!("documentation: {link}");
                    }
                }
            }
        });

        self.save_prompt(&ctx);

        let title = format!(
            "{}{} — {}",
            self.display_name(),
            if self.dirty { " •" } else { "" },
            self.language.name
        );
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl App {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Shift variants first: a shortcut matches with extra Shift held.
        let (save_as, save, new, open, quit) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&SAVE_AS),
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&NEW),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&QUIT),
            )
        });
        if save_as {
            self.save_as();
        } else if save {
            self.save();
        }
        if new {
            self.request(Pending::New, ctx);
        }
        if open {
            self.request(Pending::Open, ctx);
        }
        if quit {
            self.request(Pending::Quit, ctx);
        }
    }

    fn menus(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let item = |text: &str, shortcut: &KeyboardShortcut| {
            Button::new(text).shortcut_text(ctx.format_shortcut(shortcut))
        };

        ui.menu_button("File", |ui| {
            if ui.add(item("New", &NEW)).clicked() {
                self.request(Pending::New, &ctx);
            }
            if ui.add(item("Open…", &OPEN)).clicked() {
                self.request(Pending::Open, &ctx);
            }
            ui.separator();
            if ui.add(item("Save", &SAVE)).clicked() {
                self.save();
            }
            if ui.add(item("Save As…", &SAVE_AS)).clicked() {
                self.save_as();
            }
            ui.separator();
            if ui.add(item("Quit", &QUIT)).clicked() {
                self.request(Pending::Quit, &ctx);
            }
        });
        ui.menu_button("Edit", |ui| {
            ui.add_enabled(false, item("Undo", &UNDO));
            ui.add_enabled(false, item("Redo", &REDO));
        });
    }

    /// Runs `action`, first asking to save if there are unsaved changes.
    fn request(&mut self, action: Pending, ctx: &egui::Context) {
        if self.dirty {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        match action {
            Pending::New => {
                self.program = Program::new(&self.language);
                self.path = None;
                self.dirty = false;
                self.editor = BlockEditor::default();
                self.status.clear();
            }
            Pending::Open => self.open(),
            Pending::Quit => {
                self.closing = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn save_prompt(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending else {
            return;
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("save_prompt")).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.strong(format!("Save changes to {}?", self.display_name()));
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(Choice::Save);
                }
                if ui.button("Don't Save").clicked() {
                    choice = Some(Choice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(Choice::Cancel);
        }
        match choice {
            // A canceled Save As dialog cancels the whole action.
            Some(Choice::Save) => {
                self.pending = None;
                if self.save() {
                    self.perform(action, ctx);
                }
            }
            Some(Choice::Discard) => {
                self.pending = None;
                self.perform(action, ctx);
            }
            Some(Choice::Cancel) => self.pending = None,
            None => {}
        }
    }

    /// True if the program was written.
    fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(path) => self.write(path),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        let extension = self.language.file.extension.clone();
        let suggested = self
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("untitled.{extension}"));
        let Some(mut path) = self.dialog().set_file_name(suggested).save_file() else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension(&extension);
        }
        self.write(path)
    }

    fn write(&mut self, path: PathBuf) -> bool {
        match self.program.save(&path) {
            Ok(()) => {
                self.status = format!("saved {}", path.display());
                self.path = Some(path);
                self.dirty = false;
                true
            }
            Err(error) => {
                self.status = format!("could not save {}: {error}", path.display());
                false
            }
        }
    }

    fn open(&mut self) {
        let Some(path) = self.dialog().pick_file() else {
            return;
        };
        match Program::load(&path, &self.language) {
            Ok((program, warnings)) => {
                self.program = program;
                self.path = Some(path);
                self.dirty = false;
                self.editor = BlockEditor::default();
                self.status = join(&warnings);
            }
            Err(error) => self.status = format!("could not open {}: {error}", path.display()),
        }
    }

    fn dialog(&self) -> rfd::FileDialog {
        let file = &self.language.file;
        let mut dialog = rfd::FileDialog::new().add_filter(&file.description, &[&file.extension]);
        if let Some(folder) = self.path.as_deref().and_then(Path::parent)
            && !folder.as_os_str().is_empty()
        {
            dialog = dialog.set_directory(folder);
        }
        dialog
    }

    fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "Untitled".into(), |name| name.to_string_lossy().into_owned())
    }
}

fn join(items: &[impl ToString]) -> String {
    items.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
}
