//! Between an editor and whatever runs the program. In core so an interpreter
//! can implement [`Runner`] without depending on egui.
//!
//! The editor runs nothing and keeps no run state: the consumer supplies
//! status, pauses and breakpoints every frame.

use std::collections::HashSet;

use crate::ast::{Ast, Severity};
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

/// Breakpoints are the consumer's to keep or discard; the editor only
/// requests toggles.
#[derive(Debug, Clone, Default)]
pub struct DebugView {
    pub breakpoints: HashSet<BlockId>,
    /// Several is normal: one per thread.
    pub pauses: Vec<Pause>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pause {
    pub block: BlockId,
    pub label: Option<String>,
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
    fn debug_view(&self) -> DebugView;

    fn start(&mut self, program: &Program, ast: &Ast);
    fn stop(&mut self);
    fn pause(&mut self);
    fn resume(&mut self);
    fn step(&mut self);
    fn step_over(&mut self);
    fn step_into(&mut self);
    fn step_out(&mut self);

    /// A request; the next `debug_view` says what the runner decided.
    fn toggle_breakpoint(&mut self, block: BlockId);
}
