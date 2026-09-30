use block_parse::host::{Overlay, RunCommand};
use block_parse::edit::{Fragment, Target};
use block_parse::language::{Fit, LiteralKind};
use block_parse::program::{BlockId, Program};
use block_parse::Language;
use egui::{
    Align, Align2, Color32, ComboBox, CursorIcon, FontId, Frame, LayerId, Margin, Order, Pos2, Rect,
    RichText, Sense, TextEdit, UiBuilder, Vec2, pos2, vec2,
};

use crate::color::{SwatchRecipe, Swatches};

type SwatchKey = (SwatchRecipe, Vec<block_parse::CategoryColor>);
use crate::interact::{DRAG_THRESHOLD, Drag, Gesture, LiteralEdit, Press, Pressed, SnapMark};
use crate::layout::{
    Font, LABEL_SIZE, LITERAL_SIZE, Layout, Measure, PlacedSlot, Run, SNAP_RADIUS, Scene, SlotContent,
};
use crate::paint::{self, Transform};
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
    /// Resolved when the recipe or the category colors change, not per frame.
    swatches: Option<(SwatchKey, Swatches)>,
    /// The block the context menu was opened on.
    menu: Option<BlockId>,
    id: egui::Id,
}

#[derive(Debug, Clone, Default)]
pub struct EditorOptions {
    /// Blocks cannot be moved, added or typed into; the canvas still pans.
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
    /// A request; the host's next `Overlay` has the answer.
    ToggleBreakpoint(BlockId),
    /// Clicked, not dragged.
    BlockClicked(BlockId),
    /// The user asked for a block's `documentation`. The editor never opens
    /// links itself; this is the consumer's hook to open, resolve or refuse.
    OpenDocumentation { opcode: String, link: String },
}

impl Default for BlockEditor {
    fn default() -> Self {
        Self::new("block_editor")
    }
}

struct PointerInput {
    at: Option<Pos2>,
    pressed: bool,
    down: bool,
    secondary: bool,
    delta: Vec2,
    scroll: Vec2,
    zoom: f32,
}

impl BlockEditor {
    /// `id` must be unique among editors shown at once.
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            options: EditorOptions::default(),
            view: View::default(),
            gesture: Gesture::Idle,
            edit: None,
            palette_scroll: 0.0,
            swatches: None,
            menu: None,
            id: egui::Id::new(id),
        }
    }

    /// Fills the rest of `ui` with a palette on the left and the canvas.
    pub fn show(&mut self, ui: &mut egui::Ui, language: &Language, program: &mut Program) -> EditorOutput {
        self.show_with(ui, language, program, &Overlay::default())
    }

    /// As [`show`](Self::show), drawing the host's `overlay` over the blocks.
    pub fn show_with(
        &mut self,
        ui: &mut egui::Ui,
        language: &Language,
        program: &mut Program,
        overlay: &Overlay,
    ) -> EditorOutput {
        let mut output = EditorOutput::default();
        let bounds = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(bounds, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let theme = self.options.theme.clone();
        let read_only = self.options.read_only;

        let key: SwatchKey = (
            theme.swatch.clone(),
            language.categories().iter().map(|category| category.color).collect(),
        );
        if self.swatches.as_ref().is_none_or(|(cached, _)| *cached != key) {
            self.swatches = Some((key, Swatches::resolve(language, &theme.swatch)));
        }
        let swatches = match &self.swatches {
            Some((_, swatches)) => swatches.clone(),
            None => Swatches::resolve(language, &theme.swatch),
        };
        let measure = EguiMeasure(&ctx);

        let palette = Layout {
            language,
            measure: &measure,
            swatches: &swatches,
            editing: None,
            validate: false,
        }
        .palette();
        let palette_width = self
            .options
            .palette_width
            .unwrap_or(palette.width.clamp(180.0, 360.0))
            .min(bounds.width() * 0.5);
        let palette_rect = Rect::from_min_size(bounds.min, vec2(palette_width, bounds.height()));
        let canvas_rect = Rect::from_min_max(pos2(palette_rect.max.x, bounds.min.y), bounds.max);

        let input = ctx.input(|i| PointerInput {
            at: i.pointer.hover_pos(),
            pressed: i.pointer.primary_pressed(),
            down: i.pointer.primary_down(),
            secondary: i.pointer.secondary_pressed(),
            delta: i.pointer.delta(),
            scroll: i.smooth_scroll_delta,
            zoom: i.zoom_delta(),
        });
        // Only events over this editor's own layer are ours; popups, menus and
        // windows above it keep theirs.
        let over = input
            .at
            .filter(|&at| bounds.contains(at) && ctx.layer_id_at(at) == Some(ui.layer_id()));

        if let Some(at) = over {
            if palette_rect.contains(at) {
                self.palette_scroll -= input.scroll.y;
            } else if input.zoom != 1.0 {
                self.view.zoom_about(canvas_rect, at, input.zoom);
            } else {
                self.view.pan += input.scroll;
            }
        }
        self.palette_scroll = self
            .palette_scroll
            .clamp(0.0, (palette.height - palette_rect.height()).max(0.0));
        let palette_t = Transform {
            origin: palette_rect.min - vec2(0.0, self.palette_scroll),
            zoom: 1.0,
        };
        let t = self.view.transform(canvas_rect);

        let edit = self.edit.clone();
        let layout = Layout {
            language,
            measure: &measure,
            swatches: &swatches,
            editing: edit.as_ref().map(|edit| (edit.block, edit.input.as_str())),
            validate: true,
        };
        let mut scene = layout.program(program);

        if let Gesture::Pressed(press) = &self.gesture
            && input.down
            && input.at.is_some_and(|at| at.distance(press.at) > DRAG_THRESHOLD)
        {
            let Gesture::Pressed(press) = std::mem::take(&mut self.gesture) else {
                unreachable!()
            };
            self.gesture = self.start_drag(press, language, program, t, &mut output);
        }

        match std::mem::take(&mut self.gesture) {
            Gesture::Idle => {
                if let Some(at) = over.filter(|_| input.pressed) {
                    self.gesture = self.press(at, &scene, &palette, palette_rect, palette_t, t);
                }
            }
            Gesture::Pressed(press) => {
                if input.down {
                    self.gesture = Gesture::Pressed(press);
                } else if let Pressed::Block { id, .. } = press.on {
                    output.events.push(EditorEvent::BlockClicked(id));
                }
            }
            Gesture::Panning => {
                if input.down {
                    self.view.pan += input.delta;
                    self.gesture = Gesture::Panning;
                }
            }
            Gesture::Dragging(mut drag) => {
                if let Some(at) = input.at {
                    drag.head = t.canvas(at) - drag.grab_offset;
                }
                // Every frame, release included, so a quick flick still snaps.
                let run = layout.run(&drag.fragment.blocks, drag.head);
                drag.snap = find_snap(language, program, &scene, &drag.fragment, &run);
                if input.down {
                    self.gesture = Gesture::Dragging(drag);
                } else if input.at.is_some_and(|at| palette_rect.contains(at)) {
                    // Checked before the snap, so dragging out to delete never
                    // catches a seam on the way.
                    output.changed |= drag.from_canvas;
                } else {
                    drop_run(language, program, drag);
                    output.changed = true;
                }
            }
        }

        if input.secondary {
            self.menu = over
                .filter(|at| canvas_rect.contains(*at))
                .and_then(|at| scene.hit(t.canvas(at)))
                .map(|block| block.id);
        }
        if let Some(id) = self.menu {
            response.context_menu(|ui| {
                self.context_menu(ui, id, language, program, &scene, &mut output);
            });
        }

        if output.changed {
            scene = layout.program(program);
        }

        let painter = ui.painter_at(palette_rect);
        painter.rect_filled(palette_rect, 0.0, theme.palette);
        for heading in &palette.headings {
            painter.text(
                palette_t.pos(heading.at),
                Align2::LEFT_CENTER,
                &heading.text,
                FontId::proportional(13.0),
                theme.palette_heading,
            );
        }
        paint::scene(&painter, &palette.scene, palette_t, &theme, false, &Overlay::default());

        let canvas = ui.painter_at(canvas_rect);
        canvas.rect_filled(canvas_rect, 0.0, theme.canvas);
        grid(&canvas, canvas_rect, t, theme.grid);
        paint::scene(&canvas, &scene, t, &theme, !read_only, overlay);
        if let Gesture::Dragging(drag) = &self.gesture
            && let Some((_, mark)) = &drag.snap
        {
            paint::snap_mark(&canvas, mark, t, &theme);
        }
        paint::error_tags(&canvas, &scene, t, &theme);
        paint::markers(&canvas, &scene, t, &theme, overlay);

        if !read_only {
            let mut fields = ui.new_child(UiBuilder::new().max_rect(canvas_rect));
            fields.set_clip_rect(canvas_rect);
            for slot in scene.slots() {
                if let SlotContent::Literal { kind, text, .. } = &slot.content {
                    let rect = t.rect(slot.rect);
                    if canvas_rect.intersects(rect)
                        && self.literal_field(&mut fields, rect, slot, kind, text, language, program, t.zoom, &theme)
                    {
                        output.changed = true;
                    }
                }
            }
        }

        if let Gesture::Dragging(drag) = &self.gesture {
            // On top of everything and unclipped, so it stays visible over the
            // palette on the way to being deleted.
            let floating = ctx.layer_painter(LayerId::new(Order::Foreground, self.id.with("drag")));
            let run = layout.run(&drag.fragment.blocks, drag.head);
            paint::scene(&floating, &run.scene, t, &theme, false, &Overlay::default());
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else if let Some(at) = over {
            let on_palette = palette_rect.contains(at);
            let field = scene
                .slot_at(t.canvas(at))
                .filter(|_| !on_palette && !read_only)
                .and_then(|slot| match &slot.content {
                    SlotContent::Literal { kind, .. } => Some(kind),
                    SlotContent::Empty | SlotContent::Plugged(_) => None,
                });
            let on_block = if on_palette {
                palette.entry_at(palette_t.canvas(at)).is_some()
            } else {
                scene.hit(t.canvas(at)).is_some()
            };
            // Set after the fields have drawn, so this decides for all of them.
            match field {
                Some(LiteralKind::Bool | LiteralKind::Choice(_)) => {
                    ctx.set_cursor_icon(CursorIcon::PointingHand);
                }
                Some(_) => ctx.set_cursor_icon(CursorIcon::Text),
                None if on_block && !read_only => ctx.set_cursor_icon(CursorIcon::Grab),
                None => {}
            }
            if palette_rect.contains(at)
                && let Some(entry) = palette.entry_at(palette_t.canvas(at))
                && let Some(def) = language.block(&entry.opcode)
            {
                response.on_hover_ui_at_pointer(|ui| {
                    ui.strong(&def.name);
                    if let Some(description) = &def.description {
                        ui.label(description);
                    }
                    if !def.tags.is_empty() {
                        ui.weak(def.tags.join(", "));
                    }
                });
            }
        }

        output
    }

    /// What a press on `at` would pick up, without picking it up yet.
    fn press(
        &self,
        at: Pos2,
        scene: &Scene,
        palette: &crate::layout::Palette,
        palette_rect: Rect,
        palette_t: Transform,
        t: Transform,
    ) -> Gesture {
        let read_only = self.options.read_only;
        if palette_rect.contains(at) {
            return match palette.entry_at(palette_t.canvas(at)) {
                Some(entry) if !read_only => Gesture::Pressed(Press {
                    at,
                    on: Pressed::Palette {
                        opcode: entry.opcode.clone(),
                        top_left: palette_t.pos(entry.rect.min),
                    },
                }),
                _ => Gesture::Idle,
            };
        }

        let point = t.canvas(at);
        // A press on a field belongs to its widget.
        let on_field = !read_only
            && scene
                .slot_at(point)
                .is_some_and(|slot| matches!(slot.content, SlotContent::Literal { .. }));
        if on_field {
            return Gesture::Idle;
        }
        match scene.hit(point) {
            Some(hit) => Gesture::Pressed(Press {
                at,
                on: Pressed::Block {
                    id: hit.id,
                    top_left: hit.rect.min,
                },
            }),
            None => Gesture::Panning,
        }
    }

    /// Offsets are taken from the press, so the run does not jump by the
    /// threshold.
    fn start_drag(
        &self,
        press: Press,
        language: &Language,
        program: &mut Program,
        t: Transform,
        output: &mut EditorOutput,
    ) -> Gesture {
        match press.on {
            Pressed::Palette { opcode, top_left } => {
                let Some(block) = program.instantiate(language, &opcode) else {
                    return Gesture::Idle;
                };
                let grab_offset = (press.at - top_left) / t.zoom;
                Gesture::Dragging(Drag {
                    fragment: Fragment { blocks: vec![block] },
                    from_canvas: false,
                    home: None,
                    grab_offset,
                    head: t.canvas(press.at) - grab_offset,
                    snap: None,
                })
            }
            // Read-only blocks cannot move, so dragging one pans instead.
            Pressed::Block { .. } if self.options.read_only => Gesture::Panning,
            Pressed::Block { id, top_left } => {
                let home = program.home_of(id);
                match program.detach(id) {
                    Some(fragment) => {
                        output.changed = true;
                        Gesture::Dragging(Drag {
                            fragment,
                            from_canvas: true,
                            home,
                            grab_offset: t.canvas(press.at) - top_left,
                            head: top_left,
                            snap: None,
                        })
                    }
                    None => Gesture::Idle,
                }
            }
        }
    }

    /// Puts a run in hand back where it was picked up; a block from the
    /// palette is dropped. For hosts that stop showing the editor, or switch
    /// program, mid-drag. True if the program changed.
    pub fn cancel_drag(&mut self, language: &Language, program: &mut Program) -> bool {
        let Gesture::Dragging(drag) = std::mem::take(&mut self.gesture) else {
            return false;
        };
        let Some(home) = drag.home else {
            return false;
        };
        let head = [drag.head.x, drag.head.y];
        match program.attach(language, drag.fragment, home) {
            Ok(ejected) => {
                if let Some(ejected) = ejected {
                    let _ = program.attach(language, ejected, Target::Free { pos: head });
                }
            }
            // Its home is gone, say the host edited the program meanwhile.
            Err((_, fragment)) => {
                let _ = program.attach(language, fragment, Target::Free { pos: head });
            }
        }
        true
    }

    /// True while a run is in hand. It is out of the program until dropped,
    /// so hosts should not save meanwhile.
    pub fn is_dragging(&self) -> bool {
        matches!(self.gesture, Gesture::Dragging(_))
    }

    fn context_menu(
        &mut self,
        ui: &mut egui::Ui,
        id: BlockId,
        language: &Language,
        program: &mut Program,
        scene: &Scene,
        output: &mut EditorOutput,
    ) {
        let Some(block) = program.find(id) else {
            ui.close();
            return;
        };
        let def = language.block(&block.opcode);
        if let Some(def) = def {
            ui.label(RichText::new(&def.name).strong());
            ui.separator();
        }
        if !self.options.read_only {
            if ui.button("Duplicate").clicked() {
                let at = scene
                    .blocks
                    .iter()
                    .find(|placed| placed.id == id)
                    .map_or(Pos2::ZERO, |placed| placed.rect.min + vec2(24.0, 24.0));
                if let Some(copy) = program.duplicate(id) {
                    let _ = program.attach(language, copy, Target::Free { pos: [at.x, at.y] });
                    output.changed = true;
                }
                ui.close();
            }
            if ui.button("Delete block").clicked() {
                program.remove(id);
                output.changed = true;
                self.menu = None;
                ui.close();
            }
        }
        if ui.button("Toggle breakpoint").clicked() {
            output.events.push(EditorEvent::ToggleBreakpoint(id));
            ui.close();
        }
        if let Some(def) = def
            && let Some(link) = &def.documentation
            && ui.button("Documentation").clicked()
        {
            output.events.push(EditorEvent::OpenDocumentation {
                opcode: def.opcode.clone(),
                link: link.clone(),
            });
            ui.close();
        }
    }

    /// True if the program changed.
    #[allow(clippy::too_many_arguments)]
    fn literal_field(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        slot: &PlacedSlot,
        kind: &LiteralKind,
        text: &str,
        language: &Language,
        program: &mut Program,
        zoom: f32,
        theme: &Theme,
    ) -> bool {
        let id = self.id.with((slot.parent, slot.input.as_str()));
        match kind {
            LiteralKind::Bool => {
                let mut on = text == "true";
                let area = Rect::from_center_size(rect.center(), Vec2::splat(18.0));
                if ui.put(area, egui::Checkbox::without_text(&mut on)).changed() {
                    return program.set_literal(slot.parent, &slot.input, on.to_string());
                }
                false
            }
            LiteralKind::Choice(options) => {
                let mut chosen = text.to_owned();
                ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    ComboBox::from_id_salt(id)
                        .selected_text(RichText::new(text).size(LITERAL_SIZE * zoom))
                        .width(rect.width() - 8.0)
                        .show_ui(ui, |ui| {
                            for option in options {
                                ui.selectable_value(&mut chosen, option.clone(), option);
                            }
                        });
                });
                chosen != text && program.set_literal(slot.parent, &slot.input, chosen)
            }
            _ => {
                let mut buffer = text.to_owned();
                let response = ui.put(
                    rect,
                    TextEdit::singleline(&mut buffer)
                        .id(id)
                        .frame(Frame::NONE)
                        .margin(Margin::ZERO)
                        .font(FontId::proportional(LITERAL_SIZE * zoom))
                        .text_color(theme.literal_ink)
                        .horizontal_align(Align::Center)
                        // `put` stretches the field to the slot's height, and
                        // the static painting centers; this keeps them level.
                        .vertical_align(Align::Center),
                );
                if response.gained_focus() {
                    select_all(ui.ctx(), id, buffer.chars().count());
                }
                let this = |edit: &LiteralEdit| edit.block == slot.parent && edit.input == slot.input;
                if response.has_focus() {
                    self.edit = Some(LiteralEdit {
                        block: slot.parent,
                        input: slot.input.clone(),
                    });
                } else if self.edit.as_ref().is_some_and(this) {
                    self.edit = None;
                }
                if response.lost_focus() {
                    let tidied = language.normalize_literal(&slot.ty, &buffer);
                    if tidied != text {
                        return program.set_literal(slot.parent, &slot.input, tidied);
                    }
                }
                response.changed() && program.set_literal(slot.parent, &slot.input, buffer)
            }
        }
    }
}

fn find_snap(
    language: &Language,
    program: &Program,
    scene: &Scene,
    fragment: &Fragment,
    run: &Run,
) -> Option<(Target, SnapMark)> {
    let head = fragment.blocks.first()?;
    let def = language.block(&head.opcode)?;
    let head_rect = run.scene.blocks.first()?.rect;
    let mut best: Option<(f32, Target, SnapMark)> = None;
    let mut consider = |distance: f32, target: Target, mark: SnapMark| {
        if distance <= SNAP_RADIUS
            && best.as_ref().is_none_or(|(nearest, ..)| distance < *nearest)
            && program.can_attach(language, fragment, &target).is_ok()
        {
            best = Some((distance, target, mark));
        }
    };

    if let Some(output) = def.kind.output() {
        let probe = pos2(head_rect.min.x, head_rect.center().y);
        for slot in scene.slots() {
            if language.fit(output, &slot.ty) == Fit::No {
                continue;
            }
            let distance = probe.distance(pos2(slot.rect.min.x, slot.rect.center().y));
            let target = Target::Input {
                parent: slot.parent,
                input: slot.input.clone(),
            };
            let mark = SnapMark::Slot {
                rect: slot.rect,
                shape: slot.shape,
            };
            consider(distance, target, mark);
        }
    } else {
        let probe = head_rect.min;
        for seam in &scene.seams {
            let mark = SnapMark::Seam {
                at: seam.at,
                width: head_rect.width(),
            };
            consider(probe.distance(seam.at), seam.target.clone(), mark);
        }
        for stack in scene.heads.iter().filter(|stack| !stack.is_hat) {
            let at = stack.top_left - vec2(0.0, run.size.y);
            let target = Target::Above {
                head: stack.block,
                pos: [at.x, at.y],
            };
            let mark = SnapMark::Seam {
                at: stack.top_left,
                width: stack.width,
            };
            consider(probe.distance(at), target, mark);
        }
    }
    best.map(|(_, target, mark)| (target, mark))
}

/// A reporter pushed out of a slot lands just below it.
fn drop_run(language: &Language, program: &mut Program, drag: Drag) {
    let head = [drag.head.x, drag.head.y];
    let (target, eject) = match drag.snap {
        Some((target, SnapMark::Slot { rect, .. })) => (target, rect.min + vec2(16.0, 40.0)),
        Some((target, SnapMark::Seam { .. })) => (target, drag.head),
        None => (Target::Free { pos: head }, drag.head),
    };
    match program.attach(language, drag.fragment, target) {
        Ok(Some(ejected)) => {
            let _ = program.attach(language, ejected, Target::Free { pos: [eject.x, eject.y] });
        }
        Ok(None) => {}
        Err((_, fragment)) => {
            let _ = program.attach(language, fragment, Target::Free { pos: head });
        }
    }
}

fn grid(painter: &egui::Painter, area: Rect, t: Transform, color: Color32) {
    const STEP: f32 = 24.0;
    if STEP * t.zoom < 10.0 {
        return;
    }
    let (from, to) = (t.canvas(area.min), t.canvas(area.max));
    let mut x = (from.x / STEP).ceil() * STEP;
    while x <= to.x {
        let mut y = (from.y / STEP).ceil() * STEP;
        while y <= to.y {
            painter.circle_filled(t.pos(pos2(x, y)), 1.2 * t.zoom, color);
            y += STEP;
        }
        x += STEP;
    }
}

/// Selects a field's whole text as it takes focus, so typing replaces it.
fn select_all(ctx: &egui::Context, id: egui::Id, len: usize) {
    let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) else {
        return;
    };
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(len),
    )));
    state.store(ctx, id);
}

struct EguiMeasure<'a>(&'a egui::Context);

impl Measure for EguiMeasure<'_> {
    fn text_width(&self, text: &str, font: Font) -> f32 {
        let size = match font {
            Font::Label => LABEL_SIZE,
            Font::Literal => LITERAL_SIZE,
        };
        self.0.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE)
                .size()
                .x
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_and_sync<T: Send + Sync>() {}

    fn tiny() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &block_parse::Validators::new(),
        )
        .unwrap()
    }

    #[test]
    fn a_canceled_drag_puts_the_run_back() {
        let language = tiny();
        let mut program = Program::new(&language);
        let blocks = ["when_run", "print", "print"].map(|op| program.instantiate(&language, op).unwrap());
        let second = blocks[1].id;
        program.stacks.push(block_parse::Stack {
            pos: [10.0, 10.0],
            blocks: blocks.to_vec(),
        });
        let before = program.stacks.clone();

        let mut editor = BlockEditor::default();
        let home = program.home_of(second);
        let fragment = program.detach(second).unwrap();
        editor.gesture = Gesture::Dragging(Drag {
            fragment,
            from_canvas: true,
            home,
            grab_offset: Vec2::ZERO,
            head: pos2(400.0, 300.0),
            snap: None,
        });

        assert!(editor.cancel_drag(&language, &mut program));
        assert_eq!(program.stacks, before);
        assert!(!editor.is_dragging());
        assert!(!editor.cancel_drag(&language, &mut program), "nothing left to cancel");
    }

    #[test]
    fn the_editor_and_what_it_edits_can_live_in_a_bevy_resource() {
        send_and_sync::<BlockEditor>();
        send_and_sync::<Language>();
        send_and_sync::<Program>();
    }
}
