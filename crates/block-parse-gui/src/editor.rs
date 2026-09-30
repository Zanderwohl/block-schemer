use block_parse::debug::RunCommand;
use block_parse::edit::{Fragment, Target};
use block_parse::language::{Fit, LiteralKind};
use block_parse::program::{BlockId, Program};
use block_parse::Language;
use egui::{
    Align, Align2, Color32, ComboBox, CursorIcon, FontId, Frame, LayerId, Margin, Order, Pos2, Rect,
    RichText, Sense, TextEdit, UiBuilder, Vec2, pos2, vec2,
};

use crate::color::Swatches;
use crate::interact::{Drag, DragSource, Gesture, LiteralEdit, SnapMark};
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
    /// Keyed by language name; resolved when the language changes, not per
    /// frame.
    swatches: Option<(String, Swatches)>,
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
    /// A request; the consumer's next `DebugView` has the answer.
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
        let mut output = EditorOutput::default();
        let bounds = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(bounds, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let theme = self.options.theme.clone();
        let read_only = self.options.read_only;

        if self.swatches.as_ref().is_none_or(|(name, _)| *name != language.name) {
            self.swatches = Some((language.name.clone(), Swatches::resolve(language, &theme.swatch)));
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

        match std::mem::take(&mut self.gesture) {
            Gesture::Idle => {
                if let Some(at) = over.filter(|_| input.pressed) {
                    self.press(at, language, program, &scene, &palette, palette_rect, palette_t, t, &mut output);
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
                if input.down {
                    let run = layout.run(&drag.fragment.blocks, drag.head);
                    drag.snap = find_snap(language, program, &scene, &drag.fragment, &run);
                    self.gesture = Gesture::Dragging(drag);
                } else {
                    // Checked before any snap, so dragging out to delete never
                    // catches a seam on the way.
                    if !input.at.is_some_and(|at| palette_rect.contains(at)) {
                        drop_run(language, program, drag);
                    }
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
        paint::scene(&painter, &palette.scene, palette_t, &theme, false);

        let canvas = ui.painter_at(canvas_rect);
        canvas.rect_filled(canvas_rect, 0.0, theme.canvas);
        grid(&canvas, canvas_rect, t, theme.grid);
        paint::scene(&canvas, &scene, t, &theme, !read_only);
        if let Gesture::Dragging(drag) = &self.gesture
            && let Some((_, mark)) = &drag.snap
        {
            paint::snap_mark(&canvas, mark, t, &theme);
        }
        paint::error_tags(&canvas, &scene, t, &theme);

        if !read_only {
            let mut fields = ui.new_child(UiBuilder::new().max_rect(canvas_rect));
            fields.set_clip_rect(canvas_rect);
            for slot in scene.slots() {
                if let SlotContent::Literal { kind, text, .. } = &slot.content {
                    let rect = t.rect(slot.rect);
                    if canvas_rect.intersects(rect)
                        && self.literal_field(&mut fields, rect, slot, kind, text, program, t.zoom, &theme)
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
            paint::scene(&floating, &run.scene, t, &theme, false);
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else if let Some(at) = over {
            let on_block = if palette_rect.contains(at) {
                palette.entry_at(palette_t.canvas(at)).is_some()
            } else {
                scene.hit(t.canvas(at)).is_some()
            };
            if on_block && !read_only {
                ctx.set_cursor_icon(CursorIcon::Grab);
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

    #[allow(clippy::too_many_arguments)]
    fn press(
        &mut self,
        at: Pos2,
        language: &Language,
        program: &mut Program,
        scene: &Scene,
        palette: &crate::layout::Palette,
        palette_rect: Rect,
        palette_t: Transform,
        t: Transform,
        output: &mut EditorOutput,
    ) {
        let read_only = self.options.read_only;
        if palette_rect.contains(at) {
            if read_only {
                return;
            }
            let Some(entry) = palette.entry_at(palette_t.canvas(at)) else {
                return;
            };
            let Some(block) = program.instantiate(language, &entry.opcode) else {
                return;
            };
            let grab_offset = (at - palette_t.pos(entry.rect.min)) / t.zoom;
            self.gesture = Gesture::Dragging(Drag {
                fragment: Fragment { blocks: vec![block] },
                source: DragSource::Palette {
                    opcode: entry.opcode.clone(),
                },
                grab_offset,
                head: t.canvas(at) - grab_offset,
                snap: None,
            });
            return;
        }

        let point = t.canvas(at);
        // A press on a field belongs to its widget.
        let on_field = !read_only
            && scene
                .slot_at(point)
                .is_some_and(|slot| matches!(slot.content, SlotContent::Literal { .. }));
        if on_field {
            return;
        }
        match scene.hit(point).filter(|_| !read_only) {
            Some(hit) => {
                let (id, top_left) = (hit.id, hit.rect.min);
                let from = program.locate(id);
                if let Some(fragment) = program.detach(id) {
                    output.changed = true;
                    self.gesture = Gesture::Dragging(Drag {
                        fragment,
                        source: DragSource::Canvas { from },
                        grab_offset: point - top_left,
                        head: top_left,
                        snap: None,
                    });
                }
            }
            None => self.gesture = Gesture::Panning,
        }
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

    /// Puts a live widget over one literal slot. True if the program changed.
    #[allow(clippy::too_many_arguments)]
    fn literal_field(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        slot: &PlacedSlot,
        kind: &LiteralKind,
        text: &str,
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
                        .horizontal_align(Align::Center),
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
                response.changed() && program.set_literal(slot.parent, &slot.input, buffer)
            }
        }
    }
}

/// The best connection within reach for the run in hand, if any.
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

/// Releases a run: where it snapped, else free where it hangs. A reporter
/// pushed out of a slot lands just below it.
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
