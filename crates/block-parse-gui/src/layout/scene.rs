//! What layout produces: placed blocks in draw order, and the queries that
//! drawing, hit-testing and snapping share.

use std::ops::Range;

use block_parse::edit::Target;
use block_parse::language::{LiteralKind, Shape};
use block_parse::program::{BlockId, Slot};
use egui::{Pos2, Rect};

use super::is_typed;
use crate::color::Swatch;
use crate::shape::{BottomEdge, TopEdge};

/// Canvas units.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Draw order: parents first.
    pub blocks: Vec<PlacedBlock>,
    /// Ranges of `blocks`, one per stack. Only a later stack overlaps a block.
    pub stacks: Vec<Range<usize>>,
    pub seams: Vec<Seam>,
    pub heads: Vec<StackHead>,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedBlock {
    pub id: BlockId,
    /// Includes a hat's rise, not the tab.
    pub rect: Rect,
    pub form: Form,
    pub swatch: Swatch,
    pub labels: Vec<PlacedLabel>,
    pub slots: Vec<PlacedSlot>,
    /// Rows and arms, excluding branch mouths.
    pub hit: Vec<Rect>,
    /// Where the block's switch sits, if its language gives it one.
    pub switch: Option<Rect>,
    /// A later stack lies over the switch, so it is drawn but not live.
    pub switch_covered: bool,
    pub depth: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Form {
    Stack(StackForm),
    Reporter {
        shape: Shape,
        /// Sizes the ends however many rows follow: the first row's height,
        /// up to `MAX_END`.
        head: f32,
        /// The first row's center, from the top; markers point at it.
        first_row: f32,
    },
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
    /// Left-center.
    pub at: Pos2,
    pub text: String,
    pub faint: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSlot {
    pub parent: BlockId,
    /// A list's empty slot has its length as index.
    pub slot: Slot,
    pub ty: String,
    /// Shown while the slot is blank.
    pub hint: String,
    pub rect: Rect,
    pub shape: Shape,
    /// The owning block's, for empty slots and edges.
    pub swatch: Swatch,
    pub content: SlotContent,
    /// Under a later stack: drawn static, as a live widget would take that stack's clicks.
    pub covered: bool,
}

impl PlacedSlot {
    pub fn is_field(&self) -> bool {
        if self.covered {
            return false;
        }
        match &self.content {
            SlotContent::Literal { .. } => true,
            SlotContent::Append { kind } => is_typed(kind),
            SlotContent::Empty | SlotContent::Plugged(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SlotContent {
    Literal {
        kind: LiteralKind,
        text: String,
        /// The validator's message; `None` while the field has focus.
        error: Option<String>,
    },
    /// Needs a reporter.
    Empty,
    Plugged(BlockId),
    /// After a list's items, never stored. Typing into it appends, for a
    /// `kind` typed as text.
    Append { kind: LiteralKind },
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
    pub width: f32,
    pub is_hat: bool,
}

impl Scene {
    pub(super) fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            stacks: Vec::new(),
            seams: Vec::new(),
            heads: Vec::new(),
            bounds: Rect::NOTHING,
        }
    }

    pub fn slots(&self) -> impl Iterator<Item = &PlacedSlot> {
        self.blocks.iter().flat_map(|block| &block.slots)
    }

    /// The topmost block under `point`. Children draw after parents, so the
    /// last hit is the deepest.
    pub fn hit(&self, point: Pos2) -> Option<&PlacedBlock> {
        self.blocks
            .iter()
            .rev()
            .find(|block| block.hit.iter().any(|rect| rect.contains(point)))
    }

    pub fn slot_at(&self, point: Pos2) -> Option<&PlacedSlot> {
        self.slots().filter(|slot| slot.rect.contains(point)).last()
    }
}

/// Within a stack, children sit beside their parent's fields, never on them.
pub(super) fn mark_covered(scene: &mut Scene) {
    let stacks = &scene.stacks;
    let bounds: Vec<Rect> = stacks
        .iter()
        .map(|stack| {
            scene.blocks[stack.clone()]
                .iter()
                .flat_map(|block| &block.hit)
                .fold(Rect::NOTHING, |all, &hit| all.union(hit))
        })
        .collect();
    let overlaps = |a: Rect, b: Rect| a.intersect(b).is_positive();
    let covered = |rect: Rect, stack: usize| {
        (stack + 1..stacks.len()).any(|above| {
            overlaps(bounds[above], rect)
                && scene.blocks[stacks[above].clone()]
                    .iter()
                    .any(|block| block.hit.iter().any(|&hit| overlaps(hit, rect)))
        })
    };
    let mut marks = Vec::new();
    for (stack, range) in stacks.iter().enumerate() {
        for index in range.clone() {
            let block = &scene.blocks[index];
            let switch = block.switch.is_some_and(|rect| covered(rect, stack));
            let slots: Vec<bool> = block.slots.iter().map(|slot| covered(slot.rect, stack)).collect();
            marks.push((index, switch, slots));
        }
    }
    for (index, switch, slots) in marks {
        let block = &mut scene.blocks[index];
        block.switch_covered = switch;
        for (slot, covered) in block.slots.iter_mut().zip(slots) {
            slot.covered = covered;
        }
    }
}
