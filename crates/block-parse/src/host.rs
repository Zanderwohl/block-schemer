//! Between an editor and its host. In core so an interpreter can implement
//! [`Runner`] without depending on egui.
//!
//! The editor runs nothing and keeps no host state: the host supplies an
//! [`Overlay`] every frame, and requests go back out as events and commands.

use std::collections::{HashMap, HashSet};

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
pub trait Runner {
    fn status(&self) -> RunStatus;
    fn supports(&self, command: RunCommand) -> bool;
    fn overlay(&self) -> Overlay;

    fn start(&mut self, program: &Program, ast: &Ast);
    fn stop(&mut self);
    fn pause(&mut self);
    fn resume(&mut self);
    fn step(&mut self);
    fn step_over(&mut self);
    fn step_into(&mut self);
    fn step_out(&mut self);

    /// A request; the next `overlay` says what the runner decided.
    fn toggle_breakpoint(&mut self, block: BlockId);

    /// Run one block: `script` is [`Program::script_at`] for it. Anything to
    /// say back goes in the overlay's `bubbles`.
    fn run_block(&mut self, program: &Program, block: BlockId, script: &Script);
}
