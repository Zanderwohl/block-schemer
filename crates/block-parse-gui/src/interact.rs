//! Pointer and keyboard state between frames.

use block_parse::edit::{Fragment, Location, Target};
use block_parse::language::Shape;
use block_parse::program::BlockId;
use egui::{Pos2, Rect, Vec2};

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
    /// Pointer minus head top-left at the grab, canvas units, so the run does
    /// not jump.
    pub grab_offset: Vec2,
    /// Head top-left, canvas units.
    pub head: Pos2,
    /// Computed once per frame so the highlight and the drop agree.
    pub snap: Option<(Target, SnapMark)>,
}

/// Lets an interrupted drag put the run back instead of losing it.
#[derive(Debug, Clone, PartialEq)]
pub enum DragSource {
    Palette { opcode: String },
    Canvas { from: Option<Location> },
}

/// What the snap highlight draws, canvas units.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapMark {
    /// A notched top edge where the run's head would land.
    Seam { at: Pos2, width: f32 },
    Slot { rect: Rect, shape: Shape },
}

/// The literal with keyboard focus. Edits write straight into the program,
/// valid or not. Keyed by id so it survives other blocks moving.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteralEdit {
    pub block: BlockId,
    pub input: String,
}
