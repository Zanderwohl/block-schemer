//! The standalone editor window: a language, a program file and, optionally,
//! a [`Runner`] that answers runs. Behind the `app` feature.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use block_parse::History;
use block_parse::ast::Script;
use block_parse::host::{Highlight, HighlightStyle, Overlay, RunCommand, RunStatus, Runner, Tab, TabContent, TabId};
use block_parse::language::Language;
use block_parse::program::{BlockId, Program};
use eframe::egui::{self, Button, Key, KeyboardShortcut, Modifiers, ViewportCommand};

use crate::{BlockEditor, EditorEvent, RunToolbar};

#[cfg(target_os = "macos")]
mod native;

pub struct AppConfig {
    /// What eframe keys the app's saved state by, and on macOS the name in
    /// the app menu's About and Quit. The title bar shows the file and the
    /// language instead.
    pub name: String,
    pub language: Language,
    /// Opened if it exists, else written on first save.
    pub program: Option<PathBuf>,
    pub menus: Menus,
    /// Answers runs; without one, a run says there is no backend.
    pub runner: Option<Box<dyn Runner>>,
    /// The window's, shown in the taskbar or Dock while it runs. See
    /// [`icon_from_png`].
    pub icon: Option<egui::IconData>,
}

/// Where File and Edit go. Their shortcuts work whichever is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum Menus {
    /// The menu bar at the top of the screen on macOS; elsewhere, `egui`.
    #[default]
    Native,
    /// A menu bar drawn by egui at the top of the window.
    Egui,
    /// None, for hosts that provide their own.
    Hidden,
}

pub fn icon_from_png(png: &[u8]) -> Result<egui::IconData, String> {
    eframe::icon_data::from_png_bytes(png).map_err(|error| error.to_string())
}

/// Opens the window and blocks until it closes.
pub fn run(config: AppConfig) -> ExitCode {
    let mut status = String::new();
    let program = match &config.program {
        Some(path) if path.exists() => match Program::load(path, &config.language) {
            Ok((program, warnings)) => {
                status = join(&warnings);
                program
            }
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        },
        _ => Program::new(&config.language),
    };

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]);
    if let Some(icon) = config.icon {
        viewport = viewport.with_icon(icon);
    }
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut options = eframe::NativeOptions {
        viewport,
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

    let name = config.name.clone();
    let mut app = App {
        language: config.language,
        history: History::new(&program),
        program,
        path: config.program,
        editor: BlockEditor::default(),
        overlay: Overlay::default(),
        inspections: Vec::new(),
        runner: config.runner,
        dirty: false,
        status,
        menus: match config.menus {
            Menus::Native if !cfg!(target_os = "macos") => Menus::Egui,
            menus => menus,
        },
        #[cfg(target_os = "macos")]
        native: None,
        pending: None,
        closing: false,
        title: String::new(),
    };
    app.sync_tabs();
    let creator: eframe::AppCreator = Box::new(move |cc| {
        #[cfg(not(target_os = "macos"))]
        let _ = cc;
        #[cfg(target_os = "macos")]
        if app.menus == Menus::Native {
            match native::NativeMenus::new(&config.name, &cc.egui_ctx) {
                Ok(menus) => app.native = Some(menus),
                Err(error) => {
                    eprintln!("native menus: {error}");
                    app.menus = Menus::Egui;
                }
            }
        }
        Ok(Box::new(app))
    });
    match eframe::run_native(&name, options, creator) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut = KeyboardShortcut::new(COMMAND_SHIFT, Key::S);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = if cfg!(target_os = "macos") {
    REDO_SHIFT
} else {
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Y)
};
/// Also accepted off macOS, where it would otherwise match Undo.
const REDO_SHIFT: KeyboardShortcut = KeyboardShortcut::new(COMMAND_SHIFT, Key::Z);
const COMMAND_SHIFT: Modifiers = Modifiers {
    shift: true,
    command: true,
    ..Modifiers::NONE
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
}

impl Command {
    #[cfg(target_os = "macos")]
    const ALL: [Command; 7] = [
        Command::New,
        Command::Open,
        Command::Save,
        Command::SaveAs,
        Command::Quit,
        Command::Undo,
        Command::Redo,
    ];

    #[cfg(target_os = "macos")]
    fn id(self) -> &'static str {
        match self {
            Command::New => "new",
            Command::Open => "open",
            Command::Save => "save",
            Command::SaveAs => "save-as",
            Command::Quit => "quit",
            Command::Undo => "undo",
            Command::Redo => "redo",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Command::New => "New",
            Command::Open => "Open…",
            Command::Save => "Save",
            Command::SaveAs => "Save As…",
            Command::Quit => "Quit",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
        }
    }

    /// The one menus show.
    fn shortcut(self) -> KeyboardShortcut {
        match self {
            Command::New => NEW,
            Command::Open => OPEN,
            Command::Save => SAVE,
            Command::SaveAs => SAVE_AS,
            Command::Quit => QUIT,
            Command::Undo => UNDO,
            Command::Redo => REDO,
        }
    }
}

/// Shift variants first: a shortcut matches with extra Shift held.
const SHORTCUTS: [(KeyboardShortcut, Command); 8] = [
    (SAVE_AS, Command::SaveAs),
    (SAVE, Command::Save),
    (NEW, Command::New),
    (OPEN, Command::Open),
    (QUIT, Command::Quit),
    (REDO_SHIFT, Command::Redo),
    (REDO, Command::Redo),
    (UNDO, Command::Undo),
];

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

struct App {
    language: Language,
    program: Program,
    history: History,
    /// `None` until first saved or opened.
    path: Option<PathBuf>,
    editor: BlockEditor,
    /// Its tabs are rebuilt by `sync_tabs`.
    overlay: Overlay,
    inspections: Vec<(BlockId, Tab)>,
    runner: Option<Box<dyn Runner>>,
    dirty: bool,
    status: String,
    menus: Menus,
    #[cfg(target_os = "macos")]
    native: Option<native::NativeMenus>,
    pending: Option<Pending>,
    /// Set once quitting is agreed, so the close request is let through.
    closing: bool,
    title: String,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            for command in native.clicked() {
                if self.pending.is_none() {
                    self.run(command, &ctx);
                }
            }
        }
        if self.pending.is_none() {
            self.shortcuts(&ctx);
        }
        self.poll_runner();

        if self.menus == Menus::Egui {
            egui::Panel::top("menu_bar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| self.menu_bar(ui));
            });
        }
        if let Some(runner) = self.runner.as_deref() {
            let mut clicked = None;
            let mut flipped = None;
            egui::Panel::top("actions").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for toggle in runner.toggles() {
                        let mut on = toggle.on;
                        let mut response = ui.checkbox(&mut on, &toggle.label);
                        if let Some(hint) = &toggle.hint {
                            response = response.on_hover_text(hint);
                        }
                        if response.changed() {
                            flipped = Some((toggle.id, on));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let toolbar = RunToolbar {
                            status: runner.status(),
                            can_start: true,
                            supports: &|command| runner.supports(command),
                        };
                        clicked = toolbar.show(ui);
                    });
                });
            });
            if let Some(command) = clicked {
                self.command(command, &ctx);
            }
            if let Some((id, on)) = flipped
                && let Some(runner) = &mut self.runner
            {
                runner.set_toggle(&id, on);
                self.refresh_inspections();
            }
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
            let output = self.editor.show_with(ui, &self.language, &mut self.program, &self.overlay);
            if output.changed {
                self.dirty = true;
                self.dismiss_runs();
            }
            if output.settled {
                self.history.record(&self.program);
            }
            for event in output.events {
                match event {
                    EditorEvent::OpenDocumentation { link, .. } => {
                        if link.starts_with("http://") || link.starts_with("https://") {
                            ctx.open_url(egui::OpenUrl::new_tab(link));
                        } else {
                            self.status = format!("documentation: {link}");
                        }
                    }
                    EditorEvent::BlockClicked(_) => self.dismiss_runs(),
                    EditorEvent::Run { block, script } => {
                        let (answer, pending) = match &mut self.runner {
                            Some(runner) => {
                                runner.run_block(&self.program, self.path.as_deref(), block, &script);
                                let answer = runner.overlay().bubbles.remove(&block);
                                (answer, runner.status() != RunStatus::Idle)
                            }
                            None => (Some("No backend configured.".to_owned()), false),
                        };
                        // Not when a bubble already shows the answer.
                        if let Some(console) = self.sync_tabs()
                            && answer.is_none()
                        {
                            self.show_tab(console);
                        }
                        if answer.is_some() || pending {
                            self.overlay.highlights.push(Highlight {
                                block,
                                style: HighlightStyle::Dispatched,
                                label: None,
                            });
                        }
                        if let Some(answer) = answer {
                            self.overlay.bubbles.insert(block, answer);
                        }
                    }
                    EditorEvent::Inspect { block, script } => self.inspect(block, &script),
                    // The runner's own tabs stay open.
                    EditorEvent::CloseTab(id) => {
                        self.inspections.retain(|(_, tab)| tab.id != id);
                        self.sync_tabs();
                    }
                    EditorEvent::ConsoleInput { tab, line } => {
                        if let Some(runner) = &mut self.runner {
                            runner.console_input(&tab, &line);
                            self.sync_tabs();
                        }
                    }
                    EditorEvent::ConsoleEnd(tab) => {
                        if let Some(runner) = &mut self.runner {
                            runner.console_end(&tab);
                            self.sync_tabs();
                        }
                    }
                    _ => {}
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

        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            native.set_enabled(|command| self.pending.is_none() && self.enabled(command));
        }

        // Last, so a run sent this frame counts. Answers arrive with no input
        // to wake egui.
        if self.runner.as_ref().is_some_and(|runner| runner.status() != RunStatus::Idle) {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }
}

impl App {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Before the editor draws, so a focused field's own undo never sees them.
        let commands: Vec<Command> = ctx.input_mut(|i| {
            SHORTCUTS
                .iter()
                .filter(|(shortcut, _)| i.consume_shortcut(shortcut))
                .map(|&(_, command)| command)
                .collect()
        });
        // Native menus handle their own; one reaching here was disabled.
        if self.menus == Menus::Native {
            return;
        }
        for command in commands {
            self.run(command, ctx);
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::New => self.request(Pending::New, ctx),
            Command::Open => self.request(Pending::Open, ctx),
            Command::Save => {
                self.save(ctx);
            }
            Command::SaveAs => {
                self.save_as(ctx);
            }
            Command::Quit => self.request(Pending::Quit, ctx),
            Command::Undo => self.undo(ctx),
            Command::Redo => self.redo(ctx),
        }
    }

    fn enabled(&self, command: Command) -> bool {
        match command {
            Command::Undo => self.history.can_undo(&self.program),
            Command::Redo => self.history.can_redo(&self.program),
            _ => true,
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        use Command::*;
        for (title, commands) in [
            ("File", &[Some(New), Some(Open), None, Some(Save), Some(SaveAs), None, Some(Quit)][..]),
            ("Edit", &[Some(Undo), Some(Redo)]),
        ] {
            ui.menu_button(title, |ui| {
                for command in commands {
                    let Some(command) = *command else {
                        ui.separator();
                        continue;
                    };
                    let shortcut = ui.ctx().format_shortcut(&command.shortcut());
                    let button = Button::new(command.label()).shortcut_text(shortcut);
                    if ui.add_enabled(self.enabled(command), button).clicked() {
                        self.run(command, ui.ctx());
                    }
                }
            });
        }
    }

    fn undo(&mut self, ctx: &egui::Context) {
        self.editor.commit_edit(ctx, &self.language, &mut self.program);
        if self.history.undo(&mut self.program) {
            self.after_history();
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        self.editor.commit_edit(ctx, &self.language, &mut self.program);
        // An edit still being typed is a step of its own, which ends the line.
        self.history.record(&self.program);
        if self.history.redo(&mut self.program) {
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.editor.cancel_drag();
        self.dirty = !self.history.is_saved(&self.program);
        self.dismiss_runs();
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
                self.history = History::new(&self.program);
                self.path = None;
                self.dirty = false;
                self.reset_editor();
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
                if self.save(ctx) {
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
    fn save(&mut self, ctx: &egui::Context) -> bool {
        match self.path.clone() {
            Some(path) => self.write(ctx, path),
            None => self.save_as(ctx),
        }
    }

    fn save_as(&mut self, ctx: &egui::Context) -> bool {
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
        self.write(ctx, path)
    }

    fn write(&mut self, ctx: &egui::Context, path: PathBuf) -> bool {
        // So the entry is written normalized and stays one undo step.
        self.editor.commit_edit(ctx, &self.language, &mut self.program);
        match self.program.save(&path) {
            Ok(()) => {
                self.history.mark_saved(&self.program);
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
                self.history = History::new(&program);
                self.program = program;
                self.path = Some(path);
                self.dirty = false;
                self.reset_editor();
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

    /// Panels and the runner's tabs stay; inspections were of the old program.
    fn reset_editor(&mut self) {
        let options = self.editor.options.clone();
        self.editor = BlockEditor::default();
        self.editor.options = options;
        self.inspections.clear();
        self.sync_tabs();
        // Ids are only unique within a program.
        self.dismiss_runs();
    }

    fn inspection(&mut self, block: BlockId, script: &Script) -> String {
        let text = self.runner.as_mut().and_then(|runner| runner.inspect(&self.program, block, script));
        text.unwrap_or_else(|| format!("{script:#?}"))
    }

    /// For when a runner's setting changes what it shows.
    fn refresh_inspections(&mut self) {
        for index in 0..self.inspections.len() {
            let block = self.inspections[index].0;
            if let Some(script) = self.program.script_at(&self.language, block) {
                let text = self.inspection(block, &script);
                self.inspections[index].1.content = TabContent::Text(text);
            }
        }
        self.sync_tabs();
    }

    fn inspect(&mut self, block: BlockId, script: &Script) {
        let text = self.inspection(block, script);
        let id = TabId(format!("inspect {}", block.0));
        let title = self
            .program
            .find(block)
            .and_then(|block| self.language.block(&block.opcode))
            .map_or_else(|| "Inspect".to_owned(), |def| def.name.clone());
        let tab = Tab {
            id: id.clone(),
            title,
            closable: true,
            content: TabContent::Text(text),
        };
        match self.inspections.iter_mut().find(|(_, tab)| tab.id == id) {
            Some((_, old)) => *old = tab,
            None => self.inspections.push((block, tab)),
        }
        self.sync_tabs();
        self.show_tab(id);
    }

    fn show_tab(&mut self, id: TabId) {
        self.editor.options.active_tab = Some(id);
        self.editor.options.side_collapsed = false;
    }

    /// The console the runner has just written to, if any. One that has
    /// started waiting for input comes forward here, bubble or not, as a
    /// program waiting where no one can see it looks hung.
    fn sync_tabs(&mut self) -> Option<TabId> {
        let fresh = self.runner.as_ref().map(|runner| runner.overlay().tabs).unwrap_or_default();
        let old = |id: &TabId| self.overlay.tabs.iter().find(|old| old.id == *id);
        let written = fresh
            .iter()
            .find(|tab| matches!(tab.content, TabContent::Console { .. }) && old(&tab.id).is_some_and(|old| old != *tab))
            .map(|tab| tab.id.clone());
        let waiting = |tab: &Tab| matches!(tab.content, TabContent::Console { waiting: true, .. });
        let started = fresh
            .iter()
            .find(|tab| waiting(tab) && !old(&tab.id).is_some_and(waiting))
            .map(|tab| tab.id.clone());
        self.overlay.tabs = fresh;
        self.overlay.tabs.extend(self.inspections.iter().map(|(_, tab)| tab.clone()));
        if let Some(started) = started {
            self.show_tab(started);
        }
        written
    }

    fn command(&mut self, command: RunCommand, ctx: &egui::Context) {
        let Some(runner) = &mut self.runner else {
            return;
        };
        match command {
            RunCommand::Start => {
                // So the run sees the entry being typed, recorded as its own step.
                if self.editor.commit_edit(ctx, &self.language, &mut self.program) {
                    self.dirty = true;
                    self.history.record(&self.program);
                }
                runner.start(&self.program, self.path.as_deref(), &self.program.ast(&self.language));
            }
            RunCommand::Stop => runner.stop(),
            RunCommand::Pause => runner.pause(),
            RunCommand::Continue => runner.resume(),
            RunCommand::Step => runner.step(),
            RunCommand::StepOver => runner.step_over(),
            RunCommand::StepInto => runner.step_into(),
            RunCommand::StepOut => runner.step_out(),
        }
        if let Some(console) = self.sync_tabs() {
            self.show_tab(console);
        }
    }

    /// Only blocks still outlined get their bubble, so one dismissed
    /// meanwhile stays gone.
    fn poll_runner(&mut self) {
        let Some(runner) = &mut self.runner else {
            return;
        };
        if !runner.poll() {
            return;
        }
        let bubbles = runner.overlay().bubbles;
        let idle = runner.status() == RunStatus::Idle;
        let outlined: Vec<BlockId> = self
            .overlay
            .highlights
            .iter()
            .filter(|highlight| highlight.style == HighlightStyle::Dispatched)
            .map(|highlight| highlight.block)
            .collect();
        for block in outlined {
            if let Some(bubble) = bubbles.get(&block) {
                self.overlay.bubbles.insert(block, bubble.clone());
            }
        }
        if idle {
            let bubbles = &self.overlay.bubbles;
            self.overlay
                .highlights
                .retain(|highlight| highlight.style != HighlightStyle::Dispatched || bubbles.contains_key(&highlight.block));
        }
        // Not brought forward: a run that answers only in the console wrote to
        // it as it was sent, which already did.
        self.sync_tabs();
    }

    fn dismiss_runs(&mut self) {
        self.overlay.bubbles.clear();
        self.overlay.highlights.retain(|h| h.style != HighlightStyle::Dispatched);
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
