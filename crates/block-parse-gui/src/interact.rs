//! Pointer and keyboard state between frames.

use block_parse::edit::{Fragment, Location, Target};
use block_parse::program::BlockId;
use egui::{Pos2, Vec2};

#[derive(Debug, Clone, Default)]
pub enum Gesture {
    #[default]
    Idle,
    Dragging(Drag),
    Panning,
}

/// The fragment is taken out of the program on press, so palette and canvas
/// drags land through the same code and a drop on the palette deletes.
#[derive(Debug, Clone)]
pub struct Drag {
    pub fragment: Fragment,
    pub source: DragSource,
    /// Pointer minus head top-left at the grab, so the run does not jump.
    pub grab_offset: Vec2,
    /// Head top-left, canvas units.
    pub head: Pos2,
    /// Computed once per frame so the highlight and the drop agree.
    pub snap: Option<Target>,
}

/// Lets an interrupted drag put the run back instead of losing it.
#[derive(Debug, Clone, PartialEq)]
pub enum DragSource {
    Palette { opcode: String },
    Canvas { from: Location },
}

/// The literal with keyboard focus. Text goes straight into the program on
/// every edit, invalid or not, so there is no buffer to commit. Keyed by id so
/// it survives other blocks moving.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteralEdit {
    pub block: BlockId,
    pub input: String,
}
