use block_parse::ast::Ast;
use block_parse::debug::RunCommand;
use block_parse::program::BlockId;

use crate::color::Swatches;
use crate::interact::{Gesture, LiteralEdit};
use crate::theme::Theme;
use crate::view::View;

/// Holds only view and interaction state. Program, language and debug state
/// are passed in each frame.
pub struct BlockEditor {
    pub options: EditorOptions,
    pub view: View,
    gesture: Gesture,
    edit: Option<LiteralEdit>,
    palette_scroll: f32,
    /// Resolved when the language changes, not per frame.
    swatches: Option<Swatches>,
    /// Rebuilt on change, so problems show as you type without re-parsing
    /// every frame.
    ast: Option<Ast>,
    id: egui::Id,
}

#[derive(Debug, Clone)]
pub struct EditorOptions {
    pub read_only: bool,
    /// Draw a `RunToolbar` above the canvas.
    pub toolbar: bool,
    /// Allow Start while there are error-level problems.
    pub start_with_problems: bool,
    /// `None` fits the widest block.
    pub palette_width: Option<f32>,
    pub theme: Theme,
}

#[derive(Debug, Clone, Default)]
pub struct EditorOutput {
    /// Any AST the consumer holds is stale.
    pub changed: bool,
    /// Empty when shown with a `Runner`, which already received them.
    pub commands: Vec<RunCommand>,
    pub events: Vec<EditorEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditorEvent {
    /// A request; the consumer's next `DebugView` has the answer.
    ToggleBreakpoint(BlockId),
    /// Clicked, not dragged.
    BlockClicked(BlockId),
}
