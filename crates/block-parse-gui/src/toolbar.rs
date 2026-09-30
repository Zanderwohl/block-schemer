//! Separate from the editor so the host can place it.

use block_parse::host::{RunCommand, RunStatus};

/// Emits commands; runs nothing.
pub struct RunToolbar<'a> {
    pub status: RunStatus,
    /// False when there are errors and the editor is set not to start with them.
    pub can_start: bool,
    pub supports: &'a dyn Fn(RunCommand) -> bool,
}
