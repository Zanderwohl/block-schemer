//! Program to [`Scene`]. Pure given a [`Measure`], so tests use a fixed-width
//! fake. Drawing, hit-testing and snapping all read the one scene.

use std::collections::HashMap;

use block_parse::edit::Target;
use block_parse::language::{BlockDef, BlockKind, BlockLayout, Callable, ListDef, LiteralKind, Part, Shape};
use block_parse::program::{Block, BlockId, Input, Program, Slot, Stack};
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
pub const FAINT_SIZE: f32 = 11.0;
pub const SWITCH_SIZE: f32 = 16.0;
/// Of a `Body` block's later rows, past the first row's start.
pub const BODY_INDENT: f32 = 16.0;
pub const APPEND_WIDTH: f32 = 30.0;
pub const MAX_END: f32 = 40.0;
pub const HANDLE_WIDTH: f32 = 12.0;
const HANDLE_GAP: f32 = 2.0;
/// Inside a chip, around the declaring literal and its handle.
pub const CHIP_PADDING: f32 = 3.0;
const PALETTE_MARGIN: f32 = 14.0;
const PALETTE_GAP: f32 = 10.0;
const PALETTE_HEADING: f32 = 28.0;
const GRID_GAP: f32 = 24.0;

mod calls;
mod scene;

use scene::mark_covered;
pub use calls::Declarer;
pub use scene::{
    Form, Grip, PlacedBlock, PlacedLabel, PlacedSlot, Scene, Seam, Section, SlotContent, StackForm, StackHead,
};

pub fn append_text(hint: &str) -> String {
    format!("{hint}…")
}

/// Canvas units.
pub trait Measure {
    fn text_width(&self, text: &str, font: Font) -> f32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    Label,
    Faint,
    Literal,
}

pub use block_parse::language::is_blank;

/// Typed as text, rather than ticked, chosen or not typed at all.
pub fn is_typed(kind: &LiteralKind) -> bool {
    !matches!(kind, LiteralKind::None | LiteralKind::Bool | LiteralKind::Choice(_))
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
    pub editing: Option<(BlockId, &'a Slot)>,
    /// Off for palette templates, which should never look wrong.
    pub validate: bool,
    /// The head of a run in hand, which [`program`](Self::program) lays out
    /// as if already detached.
    pub lifted: Option<BlockId>,
    /// From [`declarers`](Self::declarers): a reference, in hand too, takes
    /// its declaring block's color, blank-name text and parameters. Empty
    /// draws references in their own color.
    pub declarers: HashMap<BlockId, Declarer>,
}

impl Layout<'_> {
    /// Each block with a scope in `program`.
    pub fn declarers(language: &Language, program: &Program) -> HashMap<BlockId, Declarer> {
        let mut declarers = HashMap::new();
        program.each_block(|block| {
            if let Some(def) = language.block(&block.opcode).filter(|def| def.scope.is_some()) {
                let declarer = Declarer {
                    opcode: block.opcode.clone(),
                    parameters: block.parameter_names(def),
                };
                declarers.insert(block.id, declarer);
            }
        });
        declarers
    }


    pub fn program(&self, program: &Program) -> Scene {
        let mut scene = Scene::empty();
        for stack in &program.stacks {
            let first = scene.blocks.len();
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
            if scene.blocks.len() > first {
                scene.stacks.push(first..scene.blocks.len());
            }
        }
        mark_covered(&mut scene);
        scene
    }

    pub fn run(&self, blocks: &[Block], origin: Pos2) -> Run {
        let mut scene = Scene::empty();
        let height = self.place_sequence(blocks, origin, 0, &mut scene);
        scene.stacks.push(0..scene.blocks.len());
        let width = scene.blocks.first().map_or(0.0, |block| block.rect.width());
        Run {
            scene,
            size: vec2(width, height),
        }
    }

    /// Every block, or only those with a tag in `checked`.
    pub fn palette(&self, checked: Option<&[String]>) -> Palette {
        let mut templates = Program::default();
        let groups = self.groups(&mut templates, checked);

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
                faint: false,
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
        // Entries never overlap, so one range will do.
        scene.stacks.push(0..scene.blocks.len());
        Palette {
            scene,
            entries,
            headings,
            width: width + 2.0 * PALETTE_MARGIN,
            height: y,
        }
    }

    /// Every block in the language, or only those with a tag in `checked`,
    /// a column per category, uncategorized last.
    pub fn grid(&self, checked: Option<&[String]>) -> Program {
        let mut program = Program::new(self.language);
        let groups = self.groups(&mut program, checked);
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
    fn groups(&self, program: &mut Program, checked: Option<&[String]>) -> Vec<(Option<&str>, Vec<Block>)> {
        let categories = self.language.categories();
        let mut groups: Vec<(Option<&str>, Vec<Block>)> = categories
            .iter()
            .map(|category| (Some(category.name.as_str()), Vec::new()))
            .chain([(None, Vec::new())])
            .collect();
        let shown = |def: &&BlockDef| checked.is_none_or(|checked| def.tags.iter().any(|tag| checked.contains(tag)));
        for def in self.language.blocks().iter().filter(shown) {
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
        let swatch = self.swatch(self.declarer(block).unwrap_or(def));
        match &def.kind {
            BlockKind::Reporter(_) => self.reporter(block, def, swatch),
            BlockKind::Hat | BlockKind::Statement | BlockKind::Cap | BlockKind::HatCap => {
                self.stack_block(block, def, swatch)
            }
        }
    }

    fn swatch(&self, def: &BlockDef) -> Swatch {
        self.swatches
            .blocks
            .get(&def.opcode)
            .or_else(|| def.category.and_then(|index| self.swatches.categories.get(index)))
            .copied()
            .unwrap_or(self.swatches.uncategorized)
    }

    fn declarer(&self, block: &Block) -> Option<&BlockDef> {
        let declaration = block.refers.as_ref()?;
        self.language.block(&self.declarers.get(&declaration.block)?.opcode)
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
                faint: false,
            }],
            slots: Vec::new(),
            branches: Vec::new(),
            children: Vec::new(),
            seam_below: true,
            switch: None,
        }
    }

    fn reporter(&self, block: &Block, def: &BlockDef, swatch: Swatch) -> Laid {
        let extent = self.extent(block);
        let shape = match extent {
            Some(extent) if extent.named => Shape::Square,
            _ => def
                .kind
                .output()
                .and_then(|ty| self.language.ty(ty))
                .map_or(Shape::Round, |ty| ty.shape),
        };
        let parts = extent.map_or(&def.parts[..], |extent| def.shown_parts(extent.shown));
        let rows = self.rows(block, def, parts);
        let height_of = |row: &[Item]| {
            let inner = row.iter().map(|item| item.size().y).fold(0.0, f32::max);
            (inner + 6.0).max(REPORTER_HEIGHT)
        };
        let first = rows.first().map_or(REPORTER_HEIGHT, |row| height_of(row));
        // Capped so a row holding a tall block keeps modest ends.
        let head = first.min(MAX_END);
        let padding = match shape {
            Shape::Round => (head * 0.4).max(ROW_PADDING),
            Shape::Hexagon => head * 0.5 + 2.0,
            Shape::Square => 8.0,
        };
        let form = Form::Reporter {
            shape,
            head,
            first_row: first / 2.0,
        };
        let mut laid = Laid::new(block.id, form, swatch);
        let (mut y, mut width) = (0.0_f32, head);
        for (index, row) in rows.into_iter().enumerate() {
            let height = height_of(&row);
            let left = if index == 0 { padding } else { padding + BODY_INDENT };
            width = width.max(laid.row(row, left, y, height) + padding);
            y += height;
        }
        laid.size = vec2(width, y.max(head));
        laid
    }

    /// `parts`, which hold no branch, as rows by the block's layout. Never
    /// empty, so a block with an empty spec still has a row.
    fn rows(&self, block: &Block, def: &BlockDef, parts: &[Part]) -> Vec<Vec<Item>> {
        let inline = match def.layout {
            BlockLayout::Inline => usize::MAX,
            BlockLayout::Body(n) => n,
        };
        let mut rows = vec![Vec::new()];
        let mut labels = Vec::new();
        let mut inputs = 0;
        for part in parts {
            let slots = match part {
                Part::Label(_) => {
                    let label = self.item(block, part).into_iter();
                    // Leading labels name the block, so they head it.
                    if inputs < inline || inputs == 0 {
                        rows[0].extend(label);
                    } else {
                        labels.extend(label);
                    }
                    continue;
                }
                Part::Input(_) => self.item(block, part).into_iter().collect(),
                Part::List(list) => self.list(block, list),
                Part::Branch(_) => continue,
            };
            if inputs < inline {
                rows[0].extend(slots);
            } else {
                for slot in slots {
                    let mut row = std::mem::take(&mut labels);
                    row.push(slot);
                    rows.push(row);
                }
            }
            inputs += 1;
        }
        rows.last_mut().expect("starts with a row").extend(labels);
        rows
    }

    fn stack_block(&self, block: &Block, def: &BlockDef, swatch: Swatch) -> Laid {
        let top = if def.kind.is_hat() {
            TopEdge::Hat
        } else {
            TopEdge::Notched
        };
        let bottom = if def.kind.is_cap() {
            BottomEdge::Flat
        } else {
            BottomEdge::Tab
        };

        let mut laid = Laid::new(block.id, Form::Stack(StackForm {
            top,
            bottom,
            sections: Vec::new(),
        }), swatch);
        laid.top = Some(top);
        laid.seam_below = bottom == BottomEdge::Tab;
        let mut sections = Vec::new();
        let mut y = if top == TopEdge::Hat { HAT_RISE } else { 0.0 };
        let mut width: f32 = if top == TopEdge::Hat {
            HAT_WIDTH + 2.0 * ROW_PADDING
        } else {
            MIN_BLOCK_WIDTH
        };

        if def.layout != BlockLayout::Inline {
            for (index, mut row) in self.rows(block, def, &def.parts).into_iter().enumerate() {
                if index == 0 && def.switch {
                    row.push(Item::Switch);
                }
                let inner = row.iter().map(|item| item.size().y).fold(0.0, f32::max);
                let height = (inner + 12.0).max(ROW_HEIGHT);
                let left = if index == 0 { ROW_PADDING } else { ROW_PADDING + BODY_INDENT };
                width = width.max(laid.row(row, left, y, height) + ROW_PADDING);
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
            return laid;
        }

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
            } else if let Part::List(list) = part {
                row.extend(self.list(block, list));
                after_branch = false;
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
            // The name, fixed: a reference is edited through its declaration.
            Part::Input(input) if let Some(declaration) = &block.refers => {
                let text = block
                    .inputs
                    .get(&input.name)
                    .and_then(|stored| stored.literal.clone())
                    .filter(|text| !is_blank(text))
                    .or_else(|| self.declarer(block).map(|def| def.unnamed(&declaration.slot)))
                    .unwrap_or_default();
                Some(Item::Label {
                    width: self.measure.text_width(&text, Font::Label),
                    text,
                    faint: false,
                })
            }
            Part::Label(label) => Some(Item::Label {
                width: self.measure.text_width(&label.text, if label.faint { Font::Faint } else { Font::Label }),
                text: label.text.clone(),
                faint: label.faint,
            }),
            Part::Input(input) => Some(self.slot(
                block,
                (&input.name, &input.ty, &input.hint),
                None,
                block.inputs.get(&input.name),
            )),
            Part::List(_) | Part::Branch(_) => None,
        }
    }

    fn list(&self, block: &Block, list: &ListDef) -> Vec<Item> {
        if let Some(def) = self.language.block(&block.opcode)
            && def.callable == Some(Callable::Arguments(list.name.clone()))
            && let Some(extent) = self.extent(block)
        {
            return self.arguments(block, list, extent);
        }
        // As the list will be once the run in hand is out of it.
        let len = block.list_len(&list.name, self.lifted);
        let stored = &block.lists.get(&list.name).map(Vec::as_slice).unwrap_or(&[])[..len];
        let names = (list.name.as_str(), list.ty.as_str(), list.hint.as_str());
        let mut items: Vec<Item> = stored
            .iter()
            .enumerate()
            .map(|(index, input)| self.slot(block, names, Some(index), Some(input)))
            .collect();
        items.push(self.append(list, len));
        items
    }

    /// A list's empty slot, at `len`.
    fn append(&self, list: &ListDef, len: usize) -> Item {
        let width = self.measure.text_width(&append_text(&list.hint), Font::Literal) + 16.0;
        let ty = self.language.ty(&list.ty);
        Item::Slot {
            slot: Slot::item(list.name.clone(), len),
            ty: list.ty.clone(),
            hint: list.hint.clone(),
            shape: ty.map_or(Shape::Round, |ty| ty.shape),
            size: vec2(width.max(APPEND_WIDTH), SLOT_HEIGHT),
            content: LaidContent::Append {
                kind: ty.map_or(LiteralKind::None, |ty| ty.literal.clone()),
            },
            grip: None,
        }
    }

    /// `names` is the input's name, type and hint.
    fn slot(&self, block: &Block, names: (&str, &str, &str), item: Option<usize>, stored: Option<&Input>) -> Item {
        let (name, ty_name, hint) = names;
        let slot = Slot {
            input: name.to_owned(),
            item,
        };
        let ty = self.language.ty(ty_name);
        let shape = ty.map_or(Shape::Round, |ty| ty.shape);
        let kind = ty.map_or(LiteralKind::None, |ty| ty.literal.clone());

        let plugged = stored
            .and_then(|stored| stored.block.as_deref())
            .filter(|inner| Some(inner.id) != self.lifted);
        if let Some(inner) = plugged {
            let laid = self.block(inner);
            return Item::Slot {
                slot,
                ty: ty_name.to_owned(),
                hint: hint.to_owned(),
                shape,
                size: laid.size,
                content: LaidContent::Plugged(Box::new(laid)),
                grip: None,
            };
        }

        let text = stored
            .and_then(|stored| stored.literal.clone())
            .unwrap_or_default();
        let shown = if is_blank(&text) { hint } else { &text };
        let text_width = self.measure.text_width(shown, Font::Literal);
        let width = match (&kind, shape) {
            (LiteralKind::None, Shape::Hexagon) => (text_width + SLOT_HEIGHT + 4.0).max(40.0),
            (LiteralKind::None, _) => (text_width + 16.0).max(32.0),
            (LiteralKind::Bool, _) => 38.0,
            (LiteralKind::Choice(_), _) => text_width + 30.0,
            (_, Shape::Hexagon) => (text_width + SLOT_HEIGHT + 4.0).max(40.0),
            (_, Shape::Round) => (text_width + 16.0).max(30.0),
            (_, Shape::Square) => (text_width + 12.0).max(24.0),
        };
        let grip = self
            .language
            .block(&block.opcode)
            .and_then(|def| def.reference_for(name))
            .and_then(|opcode| self.language.block(opcode)?.kind.output())
            .filter(|_| kind != LiteralKind::None && block.refers.is_none())
            .map(|ty| self.language.ty(ty).map_or(Shape::Round, |ty| ty.shape));
        let content = if kind == LiteralKind::None {
            LaidContent::Empty
        } else {
            let focused = self.editing == Some((block.id, &slot));
            let error = if self.validate && !focused && !is_blank(&text) {
                self.language.parse_literal(ty_name, &text).err()
            } else {
                None
            };
            LaidContent::Literal { kind, text, error }
        };
        Item::Slot {
            slot,
            ty: ty_name.to_owned(),
            hint: hint.to_owned(),
            shape,
            size: vec2(width, SLOT_HEIGHT),
            content,
            grip,
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
    slot: Slot,
    hint: String,
    ty: String,
    rect: Rect,
    shape: Shape,
    content: LaidContent,
    grip: Option<Grip>,
}

enum LaidContent {
    Literal {
        kind: LiteralKind,
        text: String,
        error: Option<String>,
    },
    Empty,
    Plugged(Box<Laid>),
    Append { kind: LiteralKind },
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
        faint: bool,
    },
    Slot {
        slot: Slot,
        ty: String,
        hint: String,
        shape: Shape,
        /// Of the slot alone, without its handle.
        size: Vec2,
        content: LaidContent,
        /// The shape of the reference it drags out.
        grip: Option<Shape>,
    },
    Switch,
}

impl Item {
    fn size(&self) -> Vec2 {
        match self {
            Self::Label { width, .. } => vec2(*width, LABEL_SIZE),
            Self::Slot { size, grip: Some(shape), .. } => {
                *size + vec2(chip_inset(*shape, size.y) + HANDLE_GAP + HANDLE_WIDTH + CHIP_PADDING, 2.0 * CHIP_PADDING)
            }
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
                Item::Label { text, width, faint } => {
                    self.labels.push(PlacedLabel {
                        at: pos2(x, center),
                        text,
                        faint,
                    });
                    x += width;
                }
                Item::Slot {
                    slot,
                    ty,
                    hint,
                    shape,
                    content,
                    size: own,
                    grip,
                } => {
                    let left = x + grip.map_or(0.0, |shape| chip_inset(shape, own.y));
                    let rect = Rect::from_min_size(pos2(left, center - own.y / 2.0), own);
                    let grip = grip.map(|shape| Grip {
                        handle: Rect::from_min_size(
                            pos2(rect.max.x + HANDLE_GAP, center - own.y / 2.0),
                            vec2(HANDLE_WIDTH, own.y),
                        ),
                        chip: Rect::from_center_size(pos2(x + size.x / 2.0, center), size),
                        shape,
                    });
                    self.slots.push(LaidSlot {
                        slot,
                        hint,
                        ty,
                        rect,
                        shape,
                        content,
                        grip,
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

/// Room left of the field inside its chip, so the field's corners stay
/// inside the chip's ends.
fn chip_inset(shape: Shape, field: f32) -> f32 {
    let end = field + 2.0 * CHIP_PADDING;
    match shape {
        Shape::Round => end * 0.4,
        Shape::Hexagon => end * 0.5,
        Shape::Square => CHIP_PADDING,
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
        Form::Reporter { .. } => (laid.form.clone(), vec![rect]),
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
                faint: label.faint,
            })
            .collect(),
        slots: Vec::new(),
        hit,
        switch: laid.switch.map(|rect| rect.translate(offset)),
        switch_covered: false,
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
            LaidContent::Append { kind } => SlotContent::Append { kind: kind.clone() },
        };
        scene.blocks[index].slots.push(PlacedSlot {
            parent: laid.id,
            slot: slot.slot.clone(),
            ty: slot.ty.clone(),
            hint: slot.hint.clone(),
            rect: slot_rect,
            shape: slot.shape,
            swatch: laid.swatch,
            content,
            grip: slot.grip.map(|grip| Grip {
                handle: grip.handle.translate(offset),
                chip: grip.chip.translate(offset),
                shape: grip.shape,
            }),
            covered: false,
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
    use block_parse::program::Reach;
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

    fn scheme() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/scheme.ron"),
            &Validators::new(),
        )
        .unwrap()
    }

    fn literal(text: &str) -> block_parse::program::Input {
        block_parse::program::Input {
            literal: Some(text.into()),
            block: None,
        }
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
            declarers: Default::default(),
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
    fn faint_labels_are_measured_and_placed_as_faint() {
        /// Faint text half as wide as plain.
        struct ByFont;
        impl Measure for ByFont {
            fn text_width(&self, text: &str, font: Font) -> f32 {
                let per_char = if font == Font::Faint { 3.5 } else { 7.0 };
                per_char * text.chars().count() as f32
            }
        }
        let language = Language::from_ron(
            r#"Language(
                name: "t",
                file: (extension: "t"),
                types: { "n": (literal: Text) },
                blocks: [(id: "if", name: "If", spec: "if {a:n} _then_ {b:n}")],
            )"#,
            &Validators::new(),
        )
        .unwrap();
        let (program, ids) = with_stack(&language, &["if"]);
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let scene = Layout {
            language: &language,
            measure: &ByFont,
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
            declarers: Default::default(),
        }
        .program(&program);

        let block = placed(&scene, ids[0]);
        let faint: Vec<_> = block.labels.iter().map(|label| (label.text.as_str(), label.faint)).collect();
        assert_eq!(faint, [("if", false), ("then", true)]);
        let then = &block.labels[1];
        let next = block.slots.iter().find(|slot| slot.slot == Slot::input("b")).unwrap();
        assert_eq!(next.rect.min.x - then.at.x, 3.5 * 4.0 + ITEM_GAP);
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
                declarers: Default::default(),
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
    fn a_later_stack_covers_the_fields_under_it() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["print"]);
        let top = program.instantiate(&language, "print").unwrap();
        let top_id = top.id;
        program.stacks.push(block_parse::Stack {
            pos: [100.0, 50.0],
            blocks: vec![top],
        });

        let scene = scene_of(&language, &program);
        assert!(placed(&scene, ids[0]).slots.iter().all(|slot| slot.covered && !slot.is_field()));
        assert!(placed(&scene, top_id).slots.iter().all(|slot| !slot.covered && slot.is_field()));

        program.stacks[1].pos = [100.0, 400.0];
        let scene = scene_of(&language, &program);
        assert!(placed(&scene, ids[0]).slots.iter().all(|slot| !slot.covered));
    }

    fn scoped() -> Language {
        Language::from_ron(
            r#"Language(
                name: "scoped",
                file: (extension: "s"),
                types: { "name": (shape: Square, literal: Text), "value": (literal: Text) },
                blocks: [
                    (id: "get", name: "Get", kind: Reporter("value"), spec: "{name:name}"),
                    (
                        id: "for", name: "For", spec: "for {each:name} [body]",
                        scope: (declares: ["each"], over: ["body"], reference: "get"),
                    ),
                ],
            )"#,
            &Validators::new(),
        )
        .unwrap()
    }

    #[test]
    fn a_later_stack_covers_a_grip_under_it() {
        let language = scoped();
        let (mut program, ids) = with_stack(&language, &["for"]);
        program.set_literal(ids[0], &Slot::input("each"), "i".into());
        let scene = scene_of(&language, &program);
        let grip = placed(&scene, ids[0]).slots[0].grip.expect("a declaring literal has a grip");
        let handle = grip.handle.center();
        assert_eq!(scene.grip_at(handle).map(|slot| slot.parent), Some(ids[0]));
        assert!(scene.grip_at(placed(&scene, ids[0]).slots[0].rect.center()).is_none(), "the field is the field's");

        let top = program.instantiate(&language, "for").unwrap();
        program.stacks.push(block_parse::Stack {
            pos: [handle.x - 4.0, handle.y - 4.0],
            blocks: vec![top],
        });
        let scene = scene_of(&language, &program);
        assert!(placed(&scene, ids[0]).slots[0].covered);
        assert!(scene.grip_at(handle).is_none());
    }

    fn callable() -> Language {
        Language::from_ron(
            r#"Language(
                name: "calls",
                file: (extension: "c"),
                callable: true,
                types: {
                    "value": (shape: Round, literal: Text),
                    "name": (shape: Square, literal: Text),
                    "procedure": (shape: Square, accepts: All, by_name: true),
                },
                blocks: [
                    (id: "fold", name: "Fold", kind: Reporter("value"), spec: "fold {kons:procedure} {knil:value} {xs:value}"),
                    (id: "call", name: "Call", kind: Reporter("value"), spec: "{name:name} {args:value*}"),
                    (id: "get", name: "Get", kind: Reporter("value"), spec: "{name:name}"),
                    (
                        id: "define", name: "Define", kind: Reporter("value"), callable: false,
                        spec: "define {name:name} {params:name*} {body:value}",
                        scope: (
                            declares: ["name", "params"], over: ["body"], global: ["name"], reference: "get",
                            signature: (name: "name", parameters: "params", reference: "call"),
                        ),
                    ),
                ],
            )"#,
            &Validators::new(),
        )
        .unwrap()
    }

    fn shape(scene: &Scene, id: BlockId) -> Shape {
        match placed(scene, id).form {
            Form::Reporter { shape, .. } => shape,
            Form::Stack(_) => panic!("a reporter"),
        }
    }

    #[test]
    fn a_named_block_is_square_and_shows_only_its_name() {
        let language = callable();
        let (mut program, ids) = with_stack(&language, &["fold"]);
        let full = scene_of(&language, &program);
        assert_eq!(shape(&full, ids[0]), Shape::Round);
        assert_eq!(placed(&full, ids[0]).slots.len(), 3);

        program.find_mut(ids[0]).unwrap().reach = Some(Reach::Call(1));
        let partial = scene_of(&language, &program);
        assert_eq!(placed(&partial, ids[0]).slots.len(), 1);
        assert_eq!(shape(&partial, ids[0]), Shape::Round);

        program.find_mut(ids[0]).unwrap().reach = Some(Reach::Name);
        let named = scene_of(&language, &program);
        let fold = placed(&named, ids[0]);
        assert!(fold.slots.is_empty());
        assert_eq!(fold.labels.iter().map(|label| label.text.as_str()).collect::<Vec<_>>(), ["fold"]);
        assert_eq!(shape(&named, ids[0]), Shape::Square);
    }

    #[test]
    fn each_stop_is_wider_than_the_last() {
        let language = callable();
        let (program, ids) = with_stack(&language, &["fold"]);
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let layout = Layout {
            language: &language,
            measure: &Fixed,
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
            declarers: Default::default(),
        };
        let stops = layout.stops(program.find(ids[0]).unwrap());
        let reaches: Vec<_> = stops.iter().map(|(reach, _)| *reach).collect();
        let calls = (0..3).map(|n| Some(Reach::Call(n)));
        assert_eq!(reaches, [Some(Reach::Name)].into_iter().chain(calls).chain([None]).collect::<Vec<_>>());
        assert!(stops.windows(2).skip(1).all(|pair| pair[0].1 < pair[1].1), "{stops:?}");
    }

    #[test]
    fn a_procedure_reference_hints_each_argument_with_its_parameter() {
        let language = callable();
        let (mut program, ids) = with_stack(&language, &["define"]);
        program.set_literal(ids[0], &Slot::input("name"), "f".into());
        program.set_literal(ids[0], &Slot::item("params", 0), "x".into());
        program.set_literal(ids[0], &Slot::item("params", 1), "y".into());
        let name = block_parse::Declaration {
            block: ids[0],
            slot: Slot::input("name"),
        };
        let call = program.reference(&language, &name).unwrap();
        let call_id = call.id;
        program.find_mut(ids[0]).unwrap().inputs.get_mut("body").unwrap().block = Some(Box::new(call));

        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let scene = Layout {
            language: &language,
            measure: &Fixed,
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
            declarers: Layout::declarers(&language, &program),
        }
        .program(&program);
        let hints: Vec<_> = placed(&scene, call_id).slots.iter().map(|slot| slot.hint.as_str()).collect();
        assert_eq!(hints, ["x", "y"], "no empty slot to grow it");
    }

    #[test]
    fn invalid_literals_carry_their_message_unless_focused() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["while"]);
        program.set_literal(ids[0], &Slot::input("condition"), "maybe".into());

        let scene = scene_of(&language, &program);
        let condition = scene.slots().find(|slot| slot.slot.input == "condition").unwrap();
        let SlotContent::Literal { error, .. } = &condition.content else { panic!() };
        assert!(error.is_some());

        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let focused = Layout {
            language: &language,
            measure: &Fixed,
            swatches: &swatches,
            editing: Some((ids[0], &Slot::input("condition"))),
            validate: true,
            lifted: None,
            declarers: Default::default(),
        }
        .program(&program);
        let condition = focused.slots().find(|slot| slot.slot.input == "condition").unwrap();
        let SlotContent::Literal { error, .. } = &condition.content else { panic!() };
        assert!(error.is_none());
    }

    #[test]
    fn blank_literals_show_their_hint_not_an_error() {
        let language = tiny();
        let (mut program, ids) = with_stack(&language, &["add"]);
        let error = |program: &Program| {
            let scene = scene_of(&language, program);
            let slot = scene.slots().find(|slot| slot.slot.input == "a").unwrap();
            let SlotContent::Literal { error, .. } = &slot.content else { panic!() };
            error.clone()
        };
        program.set_literal(ids[0], &Slot::input("a"), "".into());
        let width = |program: &Program| {
            let scene = scene_of(&language, program);
            scene.slots().find(|slot| slot.slot.input == "a").unwrap().rect.width()
        };
        let hinted = width(&program);
        for blank in ["", "  "] {
            program.set_literal(ids[0], &Slot::input("a"), blank.into());
            assert_eq!(error(&program), None, "{blank:?}");
            assert_eq!(width(&program), hinted, "{blank:?} is sized for its hint");
        }
        program.set_literal(ids[0], &Slot::input("a"), "x".into());
        assert!(error(&program).is_some(), "typed text is still checked");

        let scheme = scheme();
        let (mut longer, car) = with_stack(&scheme, &["car"]);
        longer.set_literal(car[0], &Slot::input("pair"), "  ".into());
        let scene = scene_of(&scheme, &longer);
        let width = scene.slots().next().unwrap().rect.width();
        assert!((width - (7.0 * 4.0 + 16.0)).abs() < 1e-3, "spaces are sized for `pair`, not themselves");

        program.set_literal(ids[0], &Slot::input("a"), "".into());
        let codes: Vec<_> = program.ast(&language).problems().iter().map(|problem| problem.code).collect();
        assert_eq!(codes, [block_parse::ast::ProblemCode::InvalidLiteral], "the AST still reports it");
    }

    #[test]
    fn a_list_shows_its_items_then_the_slot_that_appends() {
        let language = scheme();
        let (mut program, ids) = with_stack(&language, &["add"]);
        let lists = &mut program.find_mut(ids[0]).unwrap().lists;
        lists.insert("args".into(), vec![literal("1"), Default::default(), literal("3")]);
        let scene = scene_of(&language, &program);
        let add = placed(&scene, ids[0]);

        let items: Vec<Option<usize>> = add.slots.iter().map(|slot| slot.slot.item).collect();
        assert_eq!(items, [Some(0), Some(1), Some(2), Some(3)]);
        assert!(matches!(&add.slots[1].content, SlotContent::Literal { text, .. } if text.is_empty()), "a hole");
        assert!(matches!(add.slots[3].content, SlotContent::Append { .. }));
        assert!(add.slots.windows(2).all(|pair| pair[0].rect.max.x < pair[1].rect.min.x), "inline");
        assert!(add.slots.iter().all(PlacedSlot::is_field), "items and the empty slot take typing");
    }

    #[test]
    fn a_blank_slot_is_sized_for_its_hint() {
        let language = scheme();
        let (mut program, ids) = with_stack(&language, &["add"]);
        let lists = &mut program.find_mut(ids[0]).unwrap().lists;
        lists.insert("args".into(), vec![Default::default(), literal("1")]);
        let scene = scene_of(&language, &program);
        let add = placed(&scene, ids[0]);

        assert!(add.slots.iter().all(|slot| slot.hint == "operand"));
        let width = |index: usize| add.slots[index].rect.width();
        assert!((width(0) - (7.0 * 7.0 + 16.0)).abs() < 1e-3, "the hole fits `operand`");
        assert!(width(1) < width(0), "a filled slot fits its text");
        assert!((width(2) - (7.0 * 8.0 + 16.0)).abs() < 1e-3, "the empty slot fits `operand…`");
    }

    #[test]
    fn body_puts_later_inputs_and_items_on_indented_rows() {
        let language = scheme();
        let (mut program, ids) = with_stack(&language, &["let"]);
        let lists = &mut program.find_mut(ids[0]).unwrap().lists;
        lists.insert("body".into(), vec![literal("a"), literal("b")]);
        let scene = scene_of(&language, &program);
        let block = placed(&scene, ids[0]);

        let rect = |input: &str, item: usize| {
            block.slots.iter().find(|slot| slot.slot == Slot::item(input, item)).unwrap().rect
        };
        let head = block.labels[0].at.y;
        assert_eq!(rect("bindings", 0).center().y, head, "the first input stays on the first row");
        let rows = [rect("body", 0), rect("body", 1), rect("body", 2)];
        assert!(rows.windows(2).all(|pair| pair[0].max.y <= pair[1].min.y), "{rows:?}");
        assert!(rows.iter().all(|row| row.min.x == rows[0].min.x && row.min.y > head));
        assert!(rows[0].min.x > block.labels[0].at.x, "indented");
        assert!(matches!(block.form, Form::Reporter { head, .. } if head < block.rect.height()));
    }

    #[test]
    fn a_hat_cap_has_a_hat_and_a_flat_bottom_with_no_seam() {
        let language = scheme();
        let (program, ids) = with_stack(&language, &["program"]);
        let scene = scene_of(&language, &program);
        let Form::Stack(form) = &placed(&scene, ids[0]).form else { panic!() };
        assert_eq!((form.top, form.bottom), (TopEdge::Hat, BottomEdge::Flat));
        assert!(scene.seams.iter().all(|seam| seam.target != Target::After(ids[0])), "nothing snaps below");
        assert!(scene.heads[0].is_hat, "nor above");
    }

    #[test]
    fn markers_point_at_a_tall_first_row_not_its_capped_ends() {
        let language = scheme();
        let (mut program, ids) = with_stack(&language, &["map"]);
        let lambda = program.instantiate(&language, "lambda").unwrap();
        let map = program.find_mut(ids[0]).unwrap();
        map.inputs.get_mut("procedure").unwrap().block = Some(Box::new(lambda));
        let scene = scene_of(&language, &program);
        let map = placed(&scene, ids[0]);
        let Form::Reporter { head, first_row, .. } = map.form else { panic!() };
        assert_eq!(head, MAX_END);
        assert_eq!(first_row, map.rect.height() / 2.0, "one row, so its center");
        assert!(first_row > head / 2.0);
    }

    #[test]
    fn labels_join_the_row_of_the_input_after_them_but_lead_the_block() {
        let language = scheme();
        let (program, ids) = with_stack(&language, &["if", "begin"]);
        let scene = scene_of(&language, &program);
        let row_of = |block: &PlacedBlock, label: &str| {
            block.labels.iter().find(|placed| placed.text == label).unwrap().at.y
        };
        let branch = placed(&scene, ids[0]);
        let y = |input: &str| branch.slots.iter().find(|slot| slot.slot.input == input).unwrap().rect.center().y;
        assert_eq!(row_of(branch, "if"), y("test"));
        assert_eq!(row_of(branch, "then"), y("consequent"));
        assert_eq!(row_of(branch, "else"), y("alternate"));

        let begin = placed(&scene, ids[1]);
        assert!(row_of(begin, "begin") < begin.slots[0].rect.min.y, "Body(0) still heads with its name");
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
            declarers: Default::default(),
        }
        .palette(None);

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
    fn a_filtered_palette_shows_only_blocks_with_a_checked_tag() {
        let language = Language::from_ron(
            include_str!("../../../examples/languages/strict_tiny.ron"),
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
            declarers: Default::default(),
        };
        let checked = ["arithmetic".to_owned(), "loop".to_owned()];
        let palette = layout.palette(Some(&checked));
        let mut expected: Vec<&str> = language
            .blocks()
            .iter()
            .filter(|def| def.tags.iter().any(|tag| checked.contains(tag)))
            .map(|def| def.opcode.as_str())
            .collect();
        let mut shown: Vec<&str> = palette.entries.iter().map(|entry| entry.opcode.as_str()).collect();
        expected.sort();
        shown.sort();
        assert_eq!(shown, expected);
        assert!(shown.len() < language.blocks().len() && shown.contains(&"add"));

        assert!(language.blocks().iter().any(|def| def.tags.is_empty()));
        assert!(layout.palette(Some(&[])).entries.is_empty(), "untagged blocks too are hidden");

        let mut gridded: Vec<String> = layout
            .grid(Some(&checked))
            .stacks
            .iter()
            .map(|stack| stack.blocks[0].opcode.clone())
            .collect();
        gridded.sort();
        assert_eq!(gridded, expected, "the grid filters as the palette does");
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
            declarers: Default::default(),
        };
        let program = layout.grid(None);
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
