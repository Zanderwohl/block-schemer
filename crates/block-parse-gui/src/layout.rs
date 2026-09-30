//! Program to [`Scene`]. Pure given a [`Measure`], so tests use a fixed-width
//! fake. Drawing, hit-testing and snapping all read the one scene.

use block_parse::edit::Target;
use block_parse::language::{BlockDef, BlockKind, InputDef, LiteralKind, Part, Shape};
use block_parse::program::{Block, BlockId, Program, Stack};
use block_parse::Language;
use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::color::{Swatch, Swatches};
use crate::shape::{ARM_WIDTH, BottomEdge, HAT_RISE, HAT_WIDTH, TopEdge};

pub const ROW_HEIGHT: f32 = 40.0;
pub const REPORTER_HEIGHT: f32 = 28.0;
pub const SLOT_HEIGHT: f32 = 22.0;
pub const ROW_PADDING: f32 = 10.0;
pub const ITEM_GAP: f32 = 6.0;
pub const EMPTY_BRANCH: f32 = 24.0;
/// A C-block with no text after its last branch.
pub const BOTTOM_ARM: f32 = 20.0;
pub const MIN_BLOCK_WIDTH: f32 = 64.0;
/// Measured from the run's top-left, not the pointer.
pub const SNAP_RADIUS: f32 = 28.0;
pub const LABEL_SIZE: f32 = 14.0;
pub const LITERAL_SIZE: f32 = 13.0;
pub const SWITCH_SIZE: f32 = 16.0;
const PALETTE_MARGIN: f32 = 14.0;
const PALETTE_GAP: f32 = 10.0;
const PALETTE_HEADING: f32 = 28.0;
const GRID_GAP: f32 = 24.0;

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
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Draw order: parents first.
    pub blocks: Vec<PlacedBlock>,
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
    pub depth: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Form {
    Stack(StackForm),
    Reporter(Shape),
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSlot {
    pub parent: BlockId,
    pub input: String,
    pub ty: String,
    pub rect: Rect,
    pub shape: Shape,
    /// The owning block's, for empty slots and edges.
    pub swatch: Swatch,
    pub content: SlotContent,
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

/// A laid-out run of blocks outside the program: a drag in hand.
#[derive(Debug, Clone)]
pub struct Run {
    pub scene: Scene,
    /// Head width, total height.
    pub size: Vec2,
}

#[derive(Debug, Clone)]
pub struct Palette {
    /// Palette-local coordinates, before scrolling.
    pub scene: Scene,
    pub entries: Vec<PaletteEntry>,
    pub headings: Vec<PlacedLabel>,
    /// What the widest entry needs.
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteEntry {
    pub opcode: String,
    pub rect: Rect,
}

impl Scene {
    fn empty() -> Self {
        Self {
            blocks: Vec::new(),
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

impl Palette {
    pub fn entry_at(&self, point: Pos2) -> Option<&PaletteEntry> {
        self.entries.iter().find(|entry| entry.rect.contains(point))
    }
}

pub struct Layout<'a> {
    pub language: &'a Language,
    pub measure: &'a dyn Measure,
    pub swatches: &'a Swatches,
    /// The literal being typed into. Its error waits until it loses focus.
    pub editing: Option<(BlockId, &'a str)>,
    /// Off for palette templates, which should never look wrong.
    pub validate: bool,
    /// The head of a run in hand, which [`program`](Self::program) lays out
    /// as if already detached.
    pub lifted: Option<BlockId>,
}

impl Layout<'_> {
    pub fn program(&self, program: &Program) -> Scene {
        let mut scene = Scene::empty();
        for stack in &program.stacks {
            let origin = pos2(stack.pos[0], stack.pos[1]);
            let mut y = origin.y;
            for (index, block) in self.unlifted(&stack.blocks).iter().enumerate() {
                let laid = self.block(block);
                if index == 0 {
                    scene.heads.push(StackHead {
                        block: block.id,
                        top_left: origin,
                        width: laid.size.x,
                        is_hat: laid.top == Some(TopEdge::Hat),
                    });
                }
                place(&laid, pos2(origin.x, y), 0, &mut scene);
                y += laid.size.y;
            }
        }
        scene
    }

    pub fn run(&self, blocks: &[Block], origin: Pos2) -> Run {
        let mut scene = Scene::empty();
        let height = self.place_sequence(blocks, origin, 0, &mut scene);
        let width = scene.blocks.first().map_or(0.0, |block| block.rect.width());
        Run {
            scene,
            size: vec2(width, height),
        }
    }

    pub fn palette(&self) -> Palette {
        let mut templates = Program::default();
        let groups = self.groups(&mut templates);

        let mut scene = Scene::empty();
        let mut entries = Vec::new();
        let mut headings = Vec::new();
        let mut y = PALETTE_MARGIN;
        let mut width: f32 = 0.0;
        for (name, members) in groups {
            if members.is_empty() {
                continue;
            }
            headings.push(PlacedLabel {
                at: pos2(PALETTE_MARGIN, y + PALETTE_HEADING / 2.0 - 4.0),
                text: name.unwrap_or("Other").to_owned(),
            });
            y += PALETTE_HEADING;
            for block in &members {
                let laid = self.block(block);
                let origin = pos2(PALETTE_MARGIN, y);
                entries.push(PaletteEntry {
                    opcode: block.opcode.clone(),
                    rect: Rect::from_min_size(origin, laid.size),
                });
                width = width.max(laid.size.x);
                y += laid.size.y + PALETTE_GAP;
                place(&laid, origin, 0, &mut scene);
            }
            y += PALETTE_GAP;
        }
        Palette {
            scene,
            entries,
            headings,
            width: width + 2.0 * PALETTE_MARGIN,
            height: y,
        }
    }

    /// Every block in the language, a column per category, uncategorized last.
    pub fn grid(&self) -> Program {
        let mut program = Program::new(self.language);
        let groups = self.groups(&mut program);
        let mut x = GRID_GAP;
        for (_, members) in groups {
            if members.is_empty() {
                continue;
            }
            let mut y = GRID_GAP;
            let mut width: f32 = 0.0;
            for block in members {
                let size = self.block(&block).size;
                program.stacks.push(Stack {
                    pos: [x, y],
                    blocks: vec![block],
                });
                width = width.max(size.x);
                y += size.y + GRID_GAP;
            }
            x += width + GRID_GAP;
        }
        program
    }

    /// Ids come from `program`. Uncategorized blocks are last, under `None`.
    fn groups(&self, program: &mut Program) -> Vec<(Option<&str>, Vec<Block>)> {
        let categories = self.language.categories();
        let mut groups: Vec<(Option<&str>, Vec<Block>)> = categories
            .iter()
            .map(|category| (Some(category.name.as_str()), Vec::new()))
            .chain([(None, Vec::new())])
            .collect();
        for def in self.language.blocks() {
            if let Some(block) = program.instantiate(self.language, &def.opcode) {
                groups[def.category.unwrap_or(categories.len())].1.push(block);
            }
        }
        groups
    }

    /// `seq` up to the run in hand, which is always its tail.
    fn unlifted<'b>(&self, seq: &'b [Block]) -> &'b [Block] {
        let end = seq.iter().position(|block| Some(block.id) == self.lifted);
        &seq[..end.unwrap_or(seq.len())]
    }

    /// Returns the total height.
    fn place_sequence(&self, blocks: &[Block], origin: Pos2, depth: u16, scene: &mut Scene) -> f32 {
        let mut y = origin.y;
        for block in blocks {
            let laid = self.block(block);
            place(&laid, pos2(origin.x, y), depth, scene);
            y += laid.size.y;
        }
        y - origin.y
    }

    fn block(&self, block: &Block) -> Laid {
        let Some(def) = self.language.block(&block.opcode) else {
            return self.unknown(block);
        };
        let swatch = self
            .swatches
            .blocks
            .get(&def.opcode)
            .or_else(|| def.category.and_then(|index| self.swatches.categories.get(index)))
            .copied()
            .unwrap_or(self.swatches.uncategorized);
        match &def.kind {
            BlockKind::Reporter(_) => self.reporter(block, def, swatch),
            BlockKind::Hat | BlockKind::Statement | BlockKind::Cap => self.stack_block(block, def, swatch),
        }
    }

    /// An opcode the language lacks: drawn so it can be seen and deleted
    /// rather than silently dropped.
    fn unknown(&self, block: &Block) -> Laid {
        let text = format!("? {}", block.opcode);
        let width = (self.measure.text_width(&text, Font::Label) + 2.0 * ROW_PADDING).max(MIN_BLOCK_WIDTH);
        Laid {
            id: block.id,
            size: vec2(width, ROW_HEIGHT),
            top: Some(TopEdge::Notched),
            form: Form::Stack(StackForm {
                top: TopEdge::Notched,
                bottom: BottomEdge::Tab,
                sections: vec![Section::Row {
                    top: 0.0,
                    bottom: ROW_HEIGHT,
                }],
            }),
            swatch: self.swatches.uncategorized,
            labels: vec![PlacedLabel {
                at: pos2(ROW_PADDING, ROW_HEIGHT / 2.0),
                text,
            }],
            slots: Vec::new(),
            branches: Vec::new(),
            children: Vec::new(),
            seam_below: true,
            switch: None,
        }
    }

    fn reporter(&self, block: &Block, def: &BlockDef, swatch: Swatch) -> Laid {
        let shape = def
            .kind
            .output()
            .and_then(|ty| self.language.ty(ty))
            .map_or(Shape::Round, |ty| ty.shape);
        let items: Vec<Item> = def
            .parts
            .iter()
            .filter_map(|part| self.item(block, part))
            .collect();
        let inner = items.iter().map(|item| item.size().y).fold(0.0, f32::max);
        let height = (inner + 6.0).max(REPORTER_HEIGHT);
        let padding = match shape {
            Shape::Round => (height * 0.4).max(ROW_PADDING),
            Shape::Hexagon => height * 0.5 + 2.0,
            Shape::Square => 8.0,
        };
        let mut laid = Laid::new(block.id, Form::Reporter(shape), swatch);
        let right = laid.row(items, padding, 0.0, height);
        laid.size = vec2((right + padding).max(height), height);
        laid
    }

    fn stack_block(&self, block: &Block, def: &BlockDef, swatch: Swatch) -> Laid {
        let top = if def.kind == BlockKind::Hat {
            TopEdge::Hat
        } else {
            TopEdge::Notched
        };
        let bottom = if def.kind == BlockKind::Cap {
            BottomEdge::Flat
        } else {
            BottomEdge::Tab
        };

        let mut laid = Laid::new(block.id, Form::Reporter(Shape::Round), swatch);
        laid.top = Some(top);
        laid.seam_below = bottom == BottomEdge::Tab;
        let mut sections = Vec::new();
        let mut y = if top == TopEdge::Hat { HAT_RISE } else { 0.0 };
        let mut width: f32 = if top == TopEdge::Hat {
            HAT_WIDTH + 2.0 * ROW_PADDING
        } else {
            MIN_BLOCK_WIDTH
        };

        let mut row: Vec<Item> = Vec::new();
        // At the end of the first row, whatever the spec puts there.
        let mut switch = def.switch;
        let mut after_branch = false;
        let mut finish_row = |laid: &mut Laid, row: Vec<Item>, y: f32, bottom_arm: bool| {
            let inner = row.iter().map(|item| item.size().y).fold(0.0, f32::max);
            let height = if row.is_empty() && bottom_arm {
                BOTTOM_ARM
            } else {
                (inner + 12.0).max(ROW_HEIGHT)
            };
            let right = laid.row(row, ROW_PADDING, y, height);
            width = width.max(right + ROW_PADDING);
            height
        };

        for part in &def.parts {
            if let Part::Branch(name) = part {
                if std::mem::take(&mut switch) {
                    row.push(Item::Switch);
                }
                let height = finish_row(&mut laid, std::mem::take(&mut row), y, false);
                sections.push(Section::Row {
                    top: y,
                    bottom: y + height,
                });
                y += height;
                let children = self.unlifted(block.branches.get(name).map(Vec::as_slice).unwrap_or(&[]));
                let mut child_y = y;
                let mut blocks = Vec::new();
                for child in children {
                    let child = self.block(child);
                    blocks.push((pos2(ARM_WIDTH, child_y), child.size.y));
                    child_y += child.size.y;
                    laid.children.push(child);
                }
                let height = (child_y - y).max(EMPTY_BRANCH);
                laid.branches.push(LaidBranch {
                    name: name.clone(),
                    top: y,
                    blocks: blocks.into_iter().map(|(at, _)| at).collect(),
                });
                sections.push(Section::Branch {
                    top: y,
                    bottom: y + height,
                });
                y += height;
                after_branch = true;
            } else if let Some(item) = self.item(block, part) {
                row.push(item);
                after_branch = false;
            }
        }
        if !row.is_empty() || sections.is_empty() || after_branch {
            if switch {
                row.push(Item::Switch);
            }
            let height = finish_row(&mut laid, row, y, after_branch);
            sections.push(Section::Row {
                top: y,
                bottom: y + height,
            });
            y += height;
        }

        laid.size = vec2(width, y);
        laid.form = Form::Stack(StackForm {
            top,
            bottom,
            sections,
        });
        laid
    }

    fn item(&self, block: &Block, part: &Part) -> Option<Item> {
        match part {
            Part::Label(text) => Some(Item::Label {
                width: self.measure.text_width(text, Font::Label),
                text: text.clone(),
            }),
            Part::Input(input) => Some(self.slot(block, input)),
            Part::Branch(_) => None,
        }
    }

    fn slot(&self, block: &Block, input: &InputDef) -> Item {
        let ty = self.language.ty(&input.ty);
        let shape = ty.map_or(Shape::Round, |ty| ty.shape);
        let kind = ty.map_or(LiteralKind::None, |ty| ty.literal.clone());
        let stored = block.inputs.get(&input.name);

        let plugged = stored
            .and_then(|stored| stored.block.as_deref())
            .filter(|inner| Some(inner.id) != self.lifted);
        if let Some(inner) = plugged {
            let laid = self.block(inner);
            return Item::Slot {
                input: input.name.clone(),
                ty: input.ty.clone(),
                shape,
                size: laid.size,
                content: LaidContent::Plugged(Box::new(laid)),
            };
        }

        let text = stored
            .and_then(|stored| stored.literal.clone())
            .unwrap_or_default();
        let text_width = self.measure.text_width(&text, Font::Literal);
        let width = match (&kind, shape) {
            (LiteralKind::None, Shape::Hexagon) => 40.0,
            (LiteralKind::None, _) => 32.0,
            (LiteralKind::Bool, _) => 38.0,
            (LiteralKind::Choice(_), _) => text_width + 30.0,
            (_, Shape::Hexagon) => (text_width + SLOT_HEIGHT + 4.0).max(40.0),
            (_, Shape::Round) => (text_width + 16.0).max(30.0),
            (_, Shape::Square) => (text_width + 12.0).max(24.0),
        };
        let content = if kind == LiteralKind::None {
            LaidContent::Empty
        } else {
            let focused = self.editing == Some((block.id, input.name.as_str()));
            let error = if self.validate && !focused {
                self.language.parse_literal(&input.ty, &text).err()
            } else {
                None
            };
            LaidContent::Literal { kind, text, error }
        };
        Item::Slot {
            input: input.name.clone(),
            ty: input.ty.clone(),
            shape,
            size: vec2(width, SLOT_HEIGHT),
            content,
        }
    }
}

/// A measured block, in its own coordinates.
struct Laid {
    id: BlockId,
    size: Vec2,
    /// `None` for reporters.
    top: Option<TopEdge>,
    form: Form,
    swatch: Swatch,
    labels: Vec<PlacedLabel>,
    slots: Vec<LaidSlot>,
    branches: Vec<LaidBranch>,
    /// Branch children, in branch order; `LaidBranch::blocks` holds where.
    children: Vec<Laid>,
    seam_below: bool,
    switch: Option<Rect>,
}

struct LaidSlot {
    input: String,
    ty: String,
    rect: Rect,
    shape: Shape,
    content: LaidContent,
}

enum LaidContent {
    Literal {
        kind: LiteralKind,
        text: String,
        error: Option<String>,
    },
    Empty,
    Plugged(Box<Laid>),
}

struct LaidBranch {
    name: String,
    top: f32,
    blocks: Vec<Pos2>,
}

enum Item {
    Label {
        text: String,
        width: f32,
    },
    Slot {
        input: String,
        ty: String,
        shape: Shape,
        size: Vec2,
        content: LaidContent,
    },
    Switch,
}

impl Item {
    fn size(&self) -> Vec2 {
        match self {
            Self::Label { width, .. } => vec2(*width, LABEL_SIZE),
            Self::Slot { size, .. } => *size,
            Self::Switch => Vec2::splat(SWITCH_SIZE),
        }
    }
}

impl Laid {
    fn new(id: BlockId, form: Form, swatch: Swatch) -> Self {
        Self {
            id,
            size: Vec2::ZERO,
            top: None,
            form,
            swatch,
            labels: Vec::new(),
            slots: Vec::new(),
            branches: Vec::new(),
            children: Vec::new(),
            seam_below: false,
            switch: None,
        }
    }

    /// Places a row's items left to right, centered in `top..top + height`.
    /// Returns where the last item ends.
    fn row(&mut self, items: Vec<Item>, left: f32, top: f32, height: f32) -> f32 {
        let mut x = left;
        let center = top + height / 2.0;
        for (index, item) in items.into_iter().enumerate() {
            if index > 0 {
                x += ITEM_GAP;
            }
            let size = item.size();
            match item {
                Item::Label { text, width } => {
                    self.labels.push(PlacedLabel {
                        at: pos2(x, center),
                        text,
                    });
                    x += width;
                }
                Item::Slot {
                    input,
                    ty,
                    shape,
                    content,
                    ..
                } => {
                    self.slots.push(LaidSlot {
                        input,
                        ty,
                        rect: Rect::from_min_size(pos2(x, center - size.y / 2.0), size),
                        shape,
                        content,
                    });
                    x += size.x;
                }
                Item::Switch => {
                    self.switch = Some(Rect::from_min_size(pos2(x, center - size.y / 2.0), size));
                    x += size.x;
                }
            }
        }
        x
    }
}

fn place(laid: &Laid, origin: Pos2, depth: u16, scene: &mut Scene) {
    let offset = origin.to_vec2();
    let rect = Rect::from_min_size(origin, laid.size);

    let (form, hit) = match &laid.form {
        Form::Stack(form) => {
            let sections: Vec<Section> = form
                .sections
                .iter()
                .map(|section| match *section {
                    Section::Row { top, bottom } => Section::Row {
                        top: top + origin.y,
                        bottom: bottom + origin.y,
                    },
                    Section::Branch { top, bottom } => Section::Branch {
                        top: top + origin.y,
                        bottom: bottom + origin.y,
                    },
                })
                .collect();
            let mut hit: Vec<Rect> = sections
                .iter()
                .map(|section| match *section {
                    Section::Row { top, bottom } => {
                        Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, bottom))
                    }
                    Section::Branch { top, bottom } => Rect::from_min_max(
                        pos2(rect.min.x, top),
                        pos2(rect.min.x + ARM_WIDTH, bottom),
                    ),
                })
                .collect();
            if form.top == TopEdge::Hat {
                hit.push(Rect::from_min_size(origin, vec2(HAT_WIDTH, HAT_RISE)));
            }
            (
                Form::Stack(StackForm {
                    top: form.top,
                    bottom: form.bottom,
                    sections,
                }),
                hit,
            )
        }
        Form::Reporter(shape) => (Form::Reporter(*shape), vec![rect]),
    };

    let index = scene.blocks.len();
    scene.bounds = scene.bounds.union(rect);
    scene.blocks.push(PlacedBlock {
        id: laid.id,
        rect,
        form,
        swatch: laid.swatch,
        labels: laid
            .labels
            .iter()
            .map(|label| PlacedLabel {
                at: label.at + offset,
                text: label.text.clone(),
            })
            .collect(),
        slots: Vec::new(),
        hit,
        switch: laid.switch.map(|rect| rect.translate(offset)),
        depth,
    });

    if laid.seam_below {
        scene.seams.push(Seam {
            target: Target::After(laid.id),
            at: pos2(rect.min.x, rect.max.y),
        });
    }

    for slot in &laid.slots {
        let slot_rect = slot.rect.translate(offset);
        let content = match &slot.content {
            LaidContent::Literal { kind, text, error } => SlotContent::Literal {
                kind: kind.clone(),
                text: text.clone(),
                error: error.clone(),
            },
            LaidContent::Empty => SlotContent::Empty,
            LaidContent::Plugged(inner) => SlotContent::Plugged(inner.id),
        };
        scene.blocks[index].slots.push(PlacedSlot {
            parent: laid.id,
            input: slot.input.clone(),
            ty: slot.ty.clone(),
            rect: slot_rect,
            shape: slot.shape,
            swatch: laid.swatch,
            content,
        });
        if let LaidContent::Plugged(inner) = &slot.content {
            place(inner, slot_rect.min, depth + 1, scene);
        }
    }

    let mut children = laid.children.iter();
    for branch in &laid.branches {
        scene.seams.push(Seam {
            target: Target::BranchStart {
                parent: laid.id,
                branch: branch.name.clone(),
            },
            at: pos2(origin.x + ARM_WIDTH, origin.y + branch.top),
        });
        for at in &branch.blocks {
            if let Some(child) = children.next() {
                place(child, *at + offset, depth + 1, scene);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::SwatchRecipe;
    use block_parse::Validators;

    /// Every character 7 units wide.
    struct Fixed;
    impl Measure for Fixed {
        fn text_width(&self, text: &str, _: Font) -> f32 {
            7.0 * text.chars().count() as f32
        }
    }

    fn tiny() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap()
    }

    fn scene_of(language: &Language, program: &Program) -> Scene {
        let swatches = Swatches::resolve(language, &SwatchRecipe::default());
        Layout {
            language,
            measure: &Fixed,
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
        }
        .program(program)
    }

    fn with_stack(language: &Language, opcodes: &[&str]) -> (Program, Vec<BlockId>) {
        let mut program = Program::new(language);
        let blocks: Vec<Block> = opcodes
            .iter()
            .map(|opcode| program.instantiate(language, opcode).unwrap())
            .collect();
        let ids = blocks.iter().map(|block| block.id).collect();
        program.stacks.push(block_parse::Stack {
            pos: [100.0, 50.0],
            blocks,
        });
        (program, ids)
    }

    fn placed(scene: &Scene, id: BlockId) -> &PlacedBlock {
        scene.blocks.iter().find(|block| block.id == id).unwrap()
    }

    #[test]
    fn stacked_blocks_share_an_edge_and_seam() {
        let language = tiny();
        let (program, ids) = with_stack(&language, &["when_run", "print", "print"]);
        let scene = scene_of(&language, &program);

        let (hat, first, second) = (placed(&scene, ids[0]), placed(&scene, ids[1]), placed(&scene, ids[2]));
        assert_eq!(hat.rect.min, pos2(100.0, 50.0));
        assert_eq!(hat.rect.max.y, first.rect.min.y);
        assert_eq!(first.rect.max.y, second.rect.min.y);
        let seam = scene
            .seams
            .iter()
            .find(|seam| seam.target == Target::After(ids[1]))
            .unwrap();
        assert_eq!(seam.at, pos2(100.0, first.rect.max.y));
    }

    #[test]
    fn branches_indent_their_blocks_by_the_arm() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["while"]);
        let inner = program.instantiate(&language, "print").unwrap();
        let inner_id = inner.id;
        program.find_mut(ids[0]).unwrap().branches.get_mut("body").unwrap().push(inner);
        let scene = scene_of(&language, &program);

        let outer = placed(&scene, ids[0]);
        let child = placed(&scene, inner_id);
        assert_eq!(child.rect.min.x, outer.rect.min.x + ARM_WIDTH);
        let Form::Stack(form) = &outer.form else { panic!() };
        let Section::Branch { top, bottom } = form.sections[1] else { panic!("{form:?}") };
        assert_eq!(child.rect.min.y, top);
        assert_eq!(bottom - top, child.rect.height());
        // A press in the branch mouth is not a press on the loop.
        let mouth = pos2(outer.rect.min.x + ARM_WIDTH + 200.0, top + 1.0);
        assert!(!outer.hit.iter().any(|rect| rect.contains(mouth)));
    }

    #[test]
    fn a_run_in_hand_lays_out_as_if_detached() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["when_run", "while", "print"]);
        let inner = program.instantiate(&language, "set").unwrap();
        let join = program.instantiate(&language, "join").unwrap();
        let (inner_id, join_id) = (inner.id, join.id);
        program.find_mut(ids[1]).unwrap().branches.get_mut("body").unwrap().push(inner);
        program.find_mut(ids[2]).unwrap().inputs.get_mut("value").unwrap().block = Some(Box::new(join));
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());

        for id in [ids[0], ids[1], ids[2], inner_id, join_id] {
            let mut detached = program.clone();
            detached.detach(id).unwrap();
            let lifted = Layout {
                language: &language,
                measure: &Fixed,
                swatches: &swatches,
                editing: None,
                validate: true,
                lifted: Some(id),
            }
            .program(&program);
            assert_eq!(lifted, scene_of(&language, &detached), "{id:?}");
        }
    }

    #[test]
    fn a_plugged_reporter_grows_its_slot_and_hides_the_literal() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["print"]);
        let before = placed(&scene_of(&language, &program), ids[0]).rect.width();

        let join = program.instantiate(&language, "join").unwrap();
        program.find_mut(ids[0]).unwrap().inputs.get_mut("value").unwrap().block = Some(Box::new(join));
        let scene = scene_of(&language, &program);
        let print = placed(&scene, ids[0]);
        assert!(print.rect.width() > before);
        assert!(matches!(print.slots[0].content, SlotContent::Plugged(_)));
    }

    #[test]
    fn invalid_literals_carry_their_message_unless_focused() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["while"]);
        program.set_literal(ids[0], "condition", "maybe".into());

        let scene = scene_of(&language, &program);
        let condition = scene.slots().find(|slot| slot.input == "condition").unwrap();
        let SlotContent::Literal { error, .. } = &condition.content else { panic!() };
        assert!(error.is_some());

        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let focused = Layout {
            language: &language,
            measure: &Fixed,
            swatches: &swatches,
            editing: Some((ids[0], "condition")),
            validate: true,
            lifted: None,
        }
        .program(&program);
        let condition = focused.slots().find(|slot| slot.input == "condition").unwrap();
        let SlotContent::Literal { error, .. } = &condition.content else { panic!() };
        assert!(error.is_none());
    }

    #[test]
    fn a_block_with_its_own_color_is_drawn_in_it_and_its_neighbors_are_not() {
        let language = Language::from_ron(
            r#"Language(
                name: "hues",
                file: (extension: "h"),
                categories: [(name: "C", color: (hue: 20.0))],
                blocks: [
                    (id: "plain", name: "Plain", category: "C", spec: "plain"),
                    (id: "odd", name: "Odd", category: "C", spec: "odd", color: Some((hue: 200.0))),
                ],
            )"#,
            &Validators::new(),
        )
        .unwrap();
        let (program, ids) = with_stack(&language, &["plain", "odd"]);
        let scene = scene_of(&language, &program);
        let recipe = SwatchRecipe::default();

        assert_eq!(placed(&scene, ids[0]).swatch, recipe.swatch(20.0, None, None));
        assert_eq!(placed(&scene, ids[1]).swatch, recipe.swatch(200.0, None, None));
    }

    #[test]
    fn a_switch_ends_the_first_row_before_any_branch() {
        let language = Language::from_ron(
            r#"Language(
                name: "codons",
                file: (extension: "c"),
                types: { "code": (literal: Text) },
                blocks: [(id: "start", name: "Start", spec: "start {code:code=AAA} [body]", switch: true)],
            )"#,
            &Validators::new(),
        )
        .unwrap();
        let (program, ids) = with_stack(&language, &["start"]);
        let scene = scene_of(&language, &program);

        let start = placed(&scene, ids[0]);
        let switch = start.switch.expect("the language gives it a switch");
        let Form::Stack(form) = &start.form else { panic!() };
        let Section::Row { top, bottom } = form.sections[0] else { panic!() };
        assert!(switch.min.y >= top && switch.max.y <= bottom, "{switch:?} is not in the first row");
        assert!(switch.min.x > start.slots[0].rect.max.x, "it follows the row's slot");
        assert!(switch.max.x <= start.rect.max.x, "and fits inside the block");
        assert!(start.hit.iter().any(|rect| rect.contains(switch.center())));
    }

    #[test]
    fn the_palette_lists_every_block_under_its_category() {
        let language = tiny();
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let palette = Layout {
            language: &language,
            measure: &Fixed,
            swatches: &swatches,
            editing: None,
            validate: false,
            lifted: None,
        }
        .palette();

        assert_eq!(palette.entries.len(), language.blocks().len());
        assert_eq!(palette.headings[0].text, "Events");
        for pair in palette.entries.windows(2) {
            assert!(pair[0].rect.max.y < pair[1].rect.min.y, "entries overlap");
        }
        for entry in &palette.entries {
            let def = language.block(&entry.opcode).unwrap();
            let expected = def
                .category
                .map_or("Other", |index| language.categories()[index].name.as_str());
            let heading = palette
                .headings
                .iter()
                .rev()
                .find(|heading| heading.at.y < entry.rect.min.y)
                .unwrap();
            assert_eq!(heading.text, expected, "{} sits under the wrong heading", entry.opcode);
        }
    }

    #[test]
    fn grid_puts_each_category_in_a_column_as_wide_as_its_widest_block() {
        // An uncategorized `print` needs a last column; the emptied Output none.
        let language = Language::from_ron(
            &include_str!("../../../examples/languages/tiny.ron").replace("category: \"Output\",", ""),
            &Validators::new(),
        )
        .unwrap();
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let layout = Layout {
            language: &language,
            measure: &Fixed,
            swatches: &swatches,
            editing: None,
            validate: false,
            lifted: None,
        };
        let program = layout.grid();
        let scene = layout.program(&program);
        assert_eq!(program.stacks.len(), language.blocks().len());

        let mut columns: Vec<(f32, Option<usize>, Vec<&PlacedBlock>)> = Vec::new();
        for stack in &program.stacks {
            let block = placed(&scene, stack.blocks[0].id);
            let category = language.block(&stack.blocks[0].opcode).unwrap().category;
            match columns.last_mut() {
                Some((x, c, members)) if *x == stack.pos[0] => {
                    assert_eq!(*c, category, "{} is in the wrong column", stack.blocks[0].opcode);
                    members.push(block);
                }
                _ => columns.push((stack.pos[0], category, vec![block])),
            }
        }
        let mut expected: Vec<Option<usize>> = (0..language.categories().len())
            .filter(|index| language.blocks().iter().any(|def| def.category == Some(*index)))
            .map(Some)
            .collect();
        expected.push(None);
        assert_eq!(columns.iter().map(|(_, c, _)| *c).collect::<Vec<_>>(), expected);

        assert_eq!(program.stacks[0].pos, [GRID_GAP, GRID_GAP]);
        for pair in columns.windows(2) {
            let widest = pair[0].2.iter().map(|block| block.rect.width()).fold(0.0, f32::max);
            assert_eq!(pair[1].0, pair[0].0 + widest + GRID_GAP);
        }
        for (_, _, members) in &columns {
            for pair in members.windows(2) {
                assert_eq!(pair[1].rect.min.y, pair[0].rect.max.y + GRID_GAP);
            }
        }
    }
}
