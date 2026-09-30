//! Pointer and keyboard state between frames.

use block_parse::edit::{Fragment, Target};
use block_parse::language::Shape;
use block_parse::program::BlockId;
use egui::{Pos2, Rect, Vec2};

/// Screen pixels the pointer must travel before a press becomes a drag, so a
/// click never takes a block out of its stack.
pub const DRAG_THRESHOLD: f32 = 4.0;

#[derive(Debug, Clone, Default)]
pub enum Gesture {
    #[default]
    Idle,
    /// Down on something draggable, not yet moved past `DRAG_THRESHOLD`.
    Pressed(Press),
    Dragging(Drag),
    Panning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    /// Screen position.
    pub at: Pos2,
    pub on: Pressed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pressed {
    /// `top_left` in screen coordinates; the palette does not zoom.
    Palette { opcode: String, top_left: Pos2 },
    /// `top_left` in canvas units.
    Block { id: BlockId, top_left: Pos2 },
}

/// The fragment is out of the program while dragged, so palette and canvas
/// drags land through the same code and a drop on the palette deletes.
#[derive(Debug, Clone)]
pub struct Drag {
    pub fragment: Fragment,
    /// Dropping a canvas run on the palette deletes it; a palette block
    /// dropped back there changes nothing.
    pub from_canvas: bool,
    /// Pointer minus head top-left at the grab, canvas units, so the run does
    /// not jump.
    pub grab_offset: Vec2,
    /// Head top-left, canvas units.
    pub head: Pos2,
    /// Computed once per frame so the highlight and the drop agree.
    pub snap: Option<(Target, SnapMark)>,
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
