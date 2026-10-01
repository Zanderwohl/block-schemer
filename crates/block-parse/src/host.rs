//! Between an editor and its host. In core so an interpreter can implement
//! [`Runner`] without depending on egui.
//!
//! The editor runs nothing and keeps no host state: the host supplies an
//! [`Overlay`] every frame, and requests go back out as events and commands.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::ast::{Ast, Script, Severity};
use crate::program::{BlockId, Program};

/// The consumer defines what each means; the editor only sends them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunCommand {
    Start,
    Stop,
    Pause,
    Continue,
    Step,
    StepOver,
    StepInto,
    StepOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunStatus {
    #[default]
    Idle,
    Running,
    Paused,
}

/// What the host wants drawn over the blocks this frame. Breakpoints are the
/// host's to keep or discard; the editor only requests toggles.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    pub breakpoints: HashSet<BlockId>,
    /// Outlines. Where one block has several, the last wins.
    pub highlights: Vec<Highlight>,
    pub annotations: Vec<Annotation>,
    /// Drawn drained of color: blocks that do not apply in the host's
    /// current context.
    pub muted: HashSet<BlockId>,
    /// State for blocks whose language gives them a `switch`. A switchable
    /// block missing here is drawn disabled.
    pub switches: HashMap<BlockId, bool>,
    /// Shown on hovering a disabled switch on the canvas: why it is disabled,
    /// which only the host knows.
    pub switch_hint: Option<String>,
    /// Speech bubbles, such as what running a block gave back.
    pub bubbles: HashMap<BlockId, String>,
    /// The side panel's tabs. The editor keeps their order, so the order
    /// here only matters among tabs new in the same frame.
    pub tabs: Vec<Tab>,
}

/// Chosen by the host; stable while its tab exists.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabId(pub String);

impl From<&str> for TabId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    /// Shows a close button, which only requests closing: the tab stays
    /// until the host stops sending it.
    pub closable: bool,
    pub content: TabContent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TabContent {
    /// Read-only, but it can be selected and copied.
    Text(String),
    /// Standard output above a line to type standard input into.
    Console { output: String },
}

/// An on/off setting a runner offers, drawn by the host.
#[derive(Debug, Clone, PartialEq)]
pub struct Toggle {
    /// The runner's own name for it, given back to `set_toggle`.
    pub id: String,
    pub label: String,
    pub on: bool,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub block: BlockId,
    pub style: HighlightStyle,
    /// Tells several apart, such as `"thread 2"` on a pause.
    pub label: Option<String>,
}

/// What a highlight means; the editor's theme picks the color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightStyle {
    Selected,
    /// Pointed at by the selection.
    Related,
    /// Running or paused here, one per thread.
    Active,
    /// Sent off by `EditorEvent::Run`, until the request is done with and
    /// its bubble, if any, is gone.
    Dispatched,
    /// An index into the theme's extra colors.
    Custom(u8),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub block: BlockId,
    pub severity: Severity,
    pub message: String,
}

/// Optional: the editor also returns commands for polling, for interpreters
/// it cannot borrow (ECS resources, other threads). A trait rather than a
/// closure per command because every command needs the interpreter mutably.
/// Everything but `overlay` and `run_block` defaults to doing nothing, so a
/// runner that only answers runs, such as a REPL, implements those two. The
/// host asks for the overlay again after each call.
pub trait Runner {
    fn overlay(&self) -> Overlay;

    fn status(&self) -> RunStatus {
        RunStatus::Idle
    }
    fn supports(&self, _command: RunCommand) -> bool {
        false
    }

    /// What it says, such as output, goes in the overlay, typically in a
    /// console tab. `path` is where the program is saved, `None` until it is.
    fn start(&mut self, _program: &Program, _path: Option<&Path>, _ast: &Ast) {}
    fn stop(&mut self) {}
    fn pause(&mut self) {}
    fn resume(&mut self) {}
    fn step(&mut self) {}
    fn step_over(&mut self) {}
    fn step_into(&mut self) {}
    fn step_out(&mut self) {}

    /// A request; the next `overlay` says what the runner decided.
    fn toggle_breakpoint(&mut self, _block: BlockId) {}

    /// Run one block: `script` is [`Program::script_at`] for it, `path` as
    /// for `start`. Anything to say back goes in the overlay's `bubbles`.
    fn run_block(&mut self, program: &Program, path: Option<&Path>, block: BlockId, script: &Script);

    /// Takes in answers to work done off the UI thread; true if `overlay`
    /// changed. The host calls it every frame and, while `status` is not
    /// `Idle`, keeps frames coming.
    fn poll(&mut self) -> bool {
        false
    }

    /// A line entered on one of the overlay's console tabs.
    fn console_input(&mut self, _tab: &TabId, _line: &str) {}

    /// Settings for the host to offer, asked for again after `set_toggle`.
    fn toggles(&self) -> Vec<Toggle> {
        Vec::new()
    }
    fn set_toggle(&mut self, _id: &str, _on: bool) {}

    /// `script`, [`Program::script_at`] `block`, as the back end would see
    /// it, such as generated source. `None` leaves it to the host.
    fn inspect(&mut self, _program: &Program, _block: BlockId, _script: &Script) -> Option<String> {
        None
    }
}
