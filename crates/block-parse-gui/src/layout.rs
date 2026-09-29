//! Program to [`Scene`]. Pure given a [`Measure`], so tests use a fixed-width
//! fake. Drawing, hit-testing and snapping all read the one scene.

use block_parse::edit::Target;
use block_parse::language::{LiteralKind, Shape};
use block_parse::program::BlockId;
use egui::{Color32, Pos2, Rect};

use crate::shape::{BottomEdge, TopEdge};

pub const ROW_HEIGHT: f32 = 40.0;
pub const REPORTER_HEIGHT: f32 = 26.0;
pub const ROW_PADDING: f32 = 10.0;
pub const ITEM_GAP: f32 = 6.0;
pub const EMPTY_BRANCH: f32 = 24.0;
/// A C-block with no text after its last branch.
pub const BOTTOM_ARM: f32 = 20.0;
/// Measured from the run's top-left, not the pointer.
pub const SNAP_RADIUS: f32 = 24.0;

/// Canvas units.
pub trait Measure {
    fn text_width(&self, text: &str, font: Font) -> f32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    Label,
    Literal,
}

/// Canvas units.
#[derive(Debug, Clone)]
pub struct Scene {
    /// Draw order: parents first.
    pub blocks: Vec<PlacedBlock>,
    pub slots: Vec<PlacedSlot>,
    pub seams: Vec<Seam>,
    pub heads: Vec<StackHead>,
    pub bounds: Rect,
}

#[derive(Debug, Clone)]
pub struct PlacedBlock {
    pub id: BlockId,
    /// Without hat rise or tab.
    pub rect: Rect,
    pub form: Form,
    pub fill: Color32,
    pub outline: Color32,
    pub ink: Color32,
    pub labels: Vec<PlacedLabel>,
    /// Rows and arms, excluding branch mouths.
    pub hit: Vec<Rect>,
    /// The deepest block under the pointer wins.
    pub depth: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Form {
    Stack(StackForm),
    Reporter(Shape),
    /// Drawn so it can be seen and deleted rather than silently dropped.
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StackForm {
    pub top: TopEdge,
    pub bottom: BottomEdge,
    /// Top to bottom.
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    Row { top: f32, bottom: f32 },
    Branch { top: f32, bottom: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabel {
    /// Left-centre.
    pub at: Pos2,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSlot {
    pub parent: BlockId,
    pub input: String,
    pub ty: String,
    pub rect: Rect,
    pub shape: Shape,
    pub content: SlotContent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SlotContent {
    Literal { kind: LiteralKind, text: String },
    /// Needs a reporter.
    Empty,
    Plugged(BlockId),
}

/// Where a statement run's top-left can connect.
#[derive(Debug, Clone, PartialEq)]
pub struct Seam {
    pub target: Target,
    pub at: Pos2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StackHead {
    pub block: BlockId,
    pub top_left: Pos2,
    pub is_hat: bool,
}
