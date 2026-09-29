//! Block outlines in canvas units, from jellycell's notch-and-tab geometry.
//! Fills are split into convex pieces: epaint fills a closed path as a
//! triangle fan, which would fill a concave notch back in.

pub const NOTCH_INSET: f32 = 14.0;
/// At the top edge; the trapezoid's wide end.
pub const NOTCH_WIDTH: f32 = 24.0;
/// Also how far the tab hangs below.
pub const NOTCH_DEPTH: f32 = 8.0;
pub const NOTCH_SLANT: f32 = 5.0;
pub const HAT_RISE: f32 = 16.0;
/// Also the indent of a branch's blocks.
pub const ARM_WIDTH: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopEdge {
    Notched,
    Hat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomEdge {
    Tab,
    /// A cap.
    Flat,
}

/// Shared by outline and fill so the two cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notch {
    pub top: f32,
    pub bottom: f32,
    pub notch_floor: f32,
    pub tab_floor: f32,
    /// x of the wide end, left and right.
    pub wide: (f32, f32),
    pub narrow: (f32, f32),
}

pub type ConvexPiece = Vec<egui::Pos2>;
