//! Pointer and keyboard state between frames.

use block_parse::edit::{Fragment, Target};
use block_parse::language::Shape;
use block_parse::program::{BlockId, Slot};
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

/// A canvas run stays in the program until dropped, so the program is always
/// whole and canceling needs nothing from it.
#[derive(Debug, Clone)]
pub struct Drag {
    /// A copy of the run in hand, for drawing and snapping.
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

#[derive(Debug, Clone, PartialEq)]
pub enum DragSource {
    /// Instantiated afresh on drop, so its ids come from the program it lands in.
    Palette { opcode: String },
    /// Dropping it on the palette deletes it.
    Canvas { head: BlockId },
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
    pub slot: Slot,
}

/// The choice literal whose menu is open. Keyed by id, like [`LiteralEdit`].
#[derive(Debug, Clone, PartialEq)]
pub struct OpenChoice {
    pub block: BlockId,
    pub slot: Slot,
    /// Screen pixels, when the menu is cut short to fit.
    pub scroll: f32,
}

impl OpenChoice {
    pub fn is(&self, block: BlockId, slot: &Slot) -> bool {
        self.block == block && self.slot == *slot
    }
}
