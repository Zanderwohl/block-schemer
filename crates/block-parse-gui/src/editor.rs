use block_parse::host::{Overlay, RunCommand};
use block_parse::edit::{Fragment, Target};
use block_parse::language::{Fit, LiteralKind};
use block_parse::program::{BlockId, Program};
use block_parse::Language;
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Frame, Key, LayerId, Margin, Modifiers, Order, Pos2,
    Rect, RichText, Sense, TextEdit, UiBuilder, Vec2, pos2, vec2,
};

use crate::color::{SwatchRecipe, Swatches};

type SwatchKey = (
    SwatchRecipe,
    Vec<block_parse::CategoryColor>,
    Vec<Option<block_parse::CategoryColor>>,
);
use crate::dropdown::Menu;
use crate::interact::{
    DRAG_THRESHOLD, Drag, DragSource, Gesture, LiteralEdit, OpenChoice, Press, Pressed, SnapMark,
};
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
    choice: Option<OpenChoice>,
    palette_scroll: f32,
    /// Resolved when the recipe or the category colors change, not per frame.
    swatches: Option<(SwatchKey, Swatches)>,
    /// The block the context menu was opened on.
    menu: Option<BlockId>,
    id: egui::Id,
}

#[derive(Debug, Clone)]
pub struct EditorOptions {
    /// Blocks cannot be moved, added or typed into; the canvas still pans.
    /// Setting it mid-drag lets any run in hand go.
    pub read_only: bool,
    /// Draw a `RunToolbar` above the canvas.
    pub toolbar: bool,
    /// Allow Start while there are error-level problems.
    pub start_with_problems: bool,
    /// Offer "Toggle breakpoint" in a block's context menu.
    pub breakpoints: bool,
    /// `None` fits the widest block.
    pub palette_width: Option<f32>,
    pub theme: Theme,
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            read_only: false,
            toolbar: false,
            start_with_problems: false,
            breakpoints: true,
            palette_width: None,
            theme: Theme::default(),
        }
    }
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
    /// A request to set a block's switch; the host's next `Overlay` has the
    /// answer. Sent in read-only mode too.
    Switched(BlockId, bool),
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
            choice: None,
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
            language.blocks().iter().map(|def| def.color).collect(),
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
            lifted: None,
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
        let mut layout = Layout {
            language,
            measure: &measure,
            swatches: &swatches,
            editing: edit.as_ref().map(|edit| (edit.block, edit.input.as_str())),
            validate: true,
            lifted: None,
        };
        if let Gesture::Pressed(press) = &self.gesture
            && input.down
            && input.at.is_some_and(|at| at.distance(press.at) > DRAG_THRESHOLD)
        {
            let Gesture::Pressed(press) = std::mem::take(&mut self.gesture) else {
                unreachable!()
            };
            self.gesture = self.start_drag(press, language, program, t);
        }
        // A drop now would edit a program the host has locked.
        if read_only {
            self.cancel_drag();
        }
        layout.lifted = self.lifted();
        let mut scene = layout.program(program);

        if read_only || self.is_dragging() {
            self.choice = None;
        }
        // Its field's own click closes it, so a press there must not.
        if input.pressed
            && let Some(open) = &self.choice
        {
            let on_menu = input.at.is_some_and(|at| ctx.layer_id_at(at) == Some(self.menu_layer()));
            let on_field = over
                .filter(|at| canvas_rect.contains(*at))
                .and_then(|at| scene.slot_at(t.canvas(at)))
                .is_some_and(|slot| open.is(slot.parent, &slot.input));
            if !on_menu && !on_field {
                self.choice = None;
            }
        }

        match std::mem::take(&mut self.gesture) {
            Gesture::Idle => {
                if let Some(at) = over.filter(|_| input.pressed) {
                    self.gesture = self.press(at, &scene, &palette, palette_rect, palette_t, t, overlay);
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
                } else {
                    // Checked before the snap, so dragging out to delete never
                    // catches a seam on the way.
                    let delete = input.at.is_some_and(|at| palette_rect.contains(at));
                    output.changed |= drop_run(language, program, drag, delete);
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
            layout.lifted = self.lifted();
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

        let mut fields = ui.new_child(UiBuilder::new().max_rect(canvas_rect));
        fields.set_clip_rect(canvas_rect);
        // Live even when read-only: switches are the host's, not program data.
        for block in &scene.blocks {
            let Some(rect) = block.switch.map(|rect| t.rect(rect)) else {
                continue;
            };
            if !canvas_rect.intersects(rect) {
                continue;
            }
            match overlay.switches.get(&block.id) {
                Some(&on) => {
                    let mut value = on;
                    if checkbox(&mut fields, rect.center(), t.zoom, &mut value).changed() {
                        output.events.push(EditorEvent::Switched(block.id, value));
                    }
                }
                // Hover only, so a press still reaches `press`, which ignores it.
                None => {
                    if let Some(hint) = &overlay.switch_hint {
                        fields
                            .interact(rect, self.id.with(("switch", block.id)), Sense::hover())
                            .on_hover_text(hint);
                    }
                }
            }
        }
        if !read_only {
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
        if self.settle_edit(&ctx, language, program) {
            output.changed = true;
        }
        if self.choice_menu(&ctx, &scene, canvas_rect, t, &theme, overlay, program) {
            output.changed = true;
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
            let on_switch = (!on_palette)
                .then(|| switch_at(&scene, overlay, t.canvas(at)))
                .flatten()
                .map(|(_, live)| live);
            let on_block = if on_palette {
                palette.entry_at(palette_t.canvas(at)).is_some()
            } else {
                scene.hit(t.canvas(at)).is_some()
            };
            // Set after the fields have drawn, so this decides for all of them.
            match field {
                _ if on_switch == Some(true) => ctx.set_cursor_icon(CursorIcon::PointingHand),
                _ if on_switch == Some(false) => ctx.set_cursor_icon(CursorIcon::NotAllowed),
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
    #[allow(clippy::too_many_arguments)]
    fn press(
        &self,
        at: Pos2,
        scene: &Scene,
        palette: &crate::layout::Palette,
        palette_rect: Rect,
        palette_t: Transform,
        t: Transform,
        overlay: &Overlay,
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
        // A switch is never a handle on its block, live or not: one without
        // state is drawn as a checkbox, and grabbing the block instead would
        // read as the checkbox being broken.
        if switch_at(scene, overlay, point).is_some() {
            return Gesture::Idle;
        }
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
    fn start_drag(&self, press: Press, language: &Language, program: &mut Program, t: Transform) -> Gesture {
        match press.on {
            Pressed::Palette { opcode, top_left } => {
                let Some(block) = program.instantiate(language, &opcode) else {
                    return Gesture::Idle;
                };
                let grab_offset = (press.at - top_left) / t.zoom;
                Gesture::Dragging(Drag {
                    fragment: Fragment { blocks: vec![block] },
                    source: DragSource::Palette { opcode },
                    grab_offset,
                    head: t.canvas(press.at) - grab_offset,
                    snap: None,
                })
            }
            // Read-only blocks cannot move, so dragging one pans instead.
            Pressed::Block { .. } if self.options.read_only => Gesture::Panning,
            Pressed::Block { id, top_left } => match program.run_at(id) {
                Some(fragment) => Gesture::Dragging(Drag {
                    fragment,
                    source: DragSource::Canvas { head: id },
                    grab_offset: t.canvas(press.at) - top_left,
                    head: top_left,
                    snap: None,
                }),
                None => Gesture::Idle,
            },
        }
    }

    /// Head of the run in hand.
    fn lifted(&self) -> Option<BlockId> {
        match &self.gesture {
            Gesture::Dragging(Drag {
                source: DragSource::Canvas { head },
                ..
            }) => Some(*head),
            _ => None,
        }
    }

    /// Hosts switching programs mid-drag call this: ids are only unique within
    /// a program, so a drop would land in the new one. True if a run was in hand.
    pub fn cancel_drag(&mut self) -> bool {
        matches!(std::mem::take(&mut self.gesture), Gesture::Dragging(_))
    }

    /// True while a run is in hand.
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
        if self.options.breakpoints && ui.button("Toggle breakpoint").clicked() {
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

    /// Ends any literal edit now, normalizing its text, rather than when the
    /// field is next drawn. Call before the program is locked, as when a run
    /// starts, so nothing depends on draw order. True if the program changed.
    pub fn commit_edit(&mut self, ctx: &egui::Context, language: &Language, program: &mut Program) -> bool {
        if let Some(edit) = &self.edit {
            let id = self.field_id(edit.block, &edit.input);
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
        self.settle_edit(ctx, language, program)
    }

    /// The open choice's menu, also its layer's id.
    fn menu_id(&self) -> egui::Id {
        self.id.with("choice")
    }

    fn menu_layer(&self) -> LayerId {
        LayerId::new(Order::Foreground, self.menu_id())
    }

    /// Shows the open choice's menu, closing it if its field has gone or
    /// scrolled away. True if the program changed.
    #[allow(clippy::too_many_arguments)]
    fn choice_menu(
        &mut self,
        ctx: &egui::Context,
        scene: &Scene,
        canvas_rect: Rect,
        t: Transform,
        theme: &Theme,
        overlay: &Overlay,
        program: &mut Program,
    ) -> bool {
        let id = self.menu_id();
        let Some(open) = &mut self.choice else {
            return false;
        };
        let field = scene
            .slots()
            .find(|slot| open.is(slot.parent, &slot.input))
            .filter(|slot| canvas_rect.intersects(t.rect(slot.rect)))
            .and_then(|slot| match &slot.content {
                SlotContent::Literal {
                    kind: LiteralKind::Choice(options),
                    text,
                    ..
                } => Some((slot, options, text)),
                _ => None,
            });
        let escape = ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
        let Some((slot, options, text)) = field.filter(|_| !escape) else {
            self.choice = None;
            return false;
        };
        let mut swatch = slot.swatch;
        if overlay.muted.contains(&slot.parent) {
            swatch.fill = swatch.muted;
            swatch.edge = swatch.muted_edge;
        }
        let menu = Menu {
            id,
            options,
            selected: text,
            swatch,
            shadow: theme.halo,
            // Readable however far out the canvas is zoomed.
            scale: t.zoom.max(1.0),
        };
        let Some(index) = menu.show(ctx, t.rect(slot.rect), &mut open.scroll) else {
            return false;
        };
        self.choice = None;
        options[index] != *text && program.set_literal(slot.parent, &slot.input, options[index].clone())
    }

    fn field_id(&self, block: BlockId, input: &str) -> egui::Id {
        self.id.with((block, input))
    }

    /// Normalizes a literal that lost focus without its field being drawn,
    /// such as one panned off the canvas or left when the editor went
    /// read-only. True if the program changed.
    fn settle_edit(&mut self, ctx: &egui::Context, language: &Language, program: &mut Program) -> bool {
        let Some(edit) = self.edit.clone() else {
            return false;
        };
        let id = self.field_id(edit.block, &edit.input);
        if ctx.memory(|memory| memory.has_focus(id)) {
            return false;
        }
        self.edit = None;
        let Some(block) = program.find(edit.block) else {
            return false;
        };
        let ty = language
            .block(&block.opcode)
            .and_then(|def| def.input(&edit.input))
            .map(|input| input.ty.clone());
        let text = block.inputs.get(&edit.input).and_then(|input| input.literal.clone());
        let (Some(ty), Some(text)) = (ty, text) else {
            return false;
        };
        let tidied = language.normalize_literal(&ty, &text);
        tidied != text && program.set_literal(edit.block, &edit.input, tidied)
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
        let id = self.field_id(slot.parent, &slot.input);
        match kind {
            LiteralKind::Bool => {
                let mut on = text == "true";
                if checkbox(ui, rect.center(), zoom, &mut on).changed() {
                    return program.set_literal(slot.parent, &slot.input, on.to_string());
                }
                false
            }
            LiteralKind::Choice(_) => {
                if ui.interact(rect, id, Sense::click()).clicked() {
                    let open = self.choice.as_ref().is_some_and(|open| open.is(slot.parent, &slot.input));
                    self.choice = (!open).then(|| OpenChoice {
                        block: slot.parent,
                        input: slot.input.clone(),
                        scroll: 0.0,
                    });
                }
                false
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
                    return tidied != text && program.set_literal(slot.parent, &slot.input, tidied);
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
            // A palette block's fresh id is in no program: `can_attach` for it.
            && program.can_move(language, fragment, &target).is_ok()
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

/// A checkbox sized to the zoom, like the disabled one painted in its place.
fn checkbox(ui: &mut egui::Ui, center: Pos2, zoom: f32, value: &mut bool) -> egui::Response {
    let size = 14.0 * zoom;
    ui.scope(|ui| {
        let spacing = ui.spacing_mut();
        spacing.icon_width = size;
        spacing.icon_width_inner = size * 0.55;
        ui.put(Rect::from_center_size(center, Vec2::splat(size)), egui::Checkbox::without_text(value))
    })
    .inner
}

/// The switch under `point`, and whether the host has given it state, so a
/// live checkbox covers it.
fn switch_at(scene: &Scene, overlay: &Overlay, point: Pos2) -> Option<(BlockId, bool)> {
    scene
        .blocks
        .iter()
        .rev()
        .find(|block| block.switch.is_some_and(|rect| rect.contains(point)))
        .map(|block| (block.id, overlay.switches.contains_key(&block.id)))
}

/// A reporter pushed out of a slot lands just below it.
/// Delete drops the run instead of placing it. A canvas run that no longer
/// matches the program, as when the host switched programs mid-drag, is left
/// alone. True if the program changed.
fn drop_run(language: &Language, program: &mut Program, drag: Drag, delete: bool) -> bool {
    let mut next = program.clone();
    let fragment = match &drag.source {
        DragSource::Canvas { head } => match next.detach(*head) {
            Some(fragment) if fragment == drag.fragment => fragment,
            _ => return false,
        },
        _ if delete => return false,
        DragSource::Palette { opcode } => match next.instantiate(language, opcode) {
            Some(block) => Fragment { blocks: vec![block] },
            None => return false,
        },
    };
    if !delete {
        let head = [drag.head.x, drag.head.y];
        let (target, eject) = match drag.snap {
            Some((target, SnapMark::Slot { rect, .. })) => (target, rect.min + vec2(16.0, 40.0)),
            Some((target, SnapMark::Seam { .. })) => (target, drag.head),
            None => (Target::Free { pos: head }, drag.head),
        };
        match next.attach(language, fragment, target) {
            Ok(Some(ejected)) => {
                let _ = next.attach(language, ejected, Target::Free { pos: [eject.x, eject.y] });
            }
            Ok(None) => {}
            Err((_, fragment)) => {
                let _ = next.attach(language, fragment, Target::Free { pos: head });
            }
        }
    }
    *program = next;
    true
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
    use crate::dropdown;

    fn send_and_sync<T: Send + Sync>() {}

    fn tiny() -> Language {
        Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &block_parse::Validators::new(),
        )
        .unwrap()
    }

    /// A stack of `when_run`, `print`, `print` at (10, 10), with the second
    /// block's run in hand.
    fn dragging_the_tail() -> (Language, Program, BlockEditor) {
        let language = tiny();
        let mut program = Program::new(&language);
        let blocks = ["when_run", "print", "print"].map(|op| program.instantiate(&language, op).unwrap());
        let second = blocks[1].id;
        program.stacks.push(block_parse::Stack {
            pos: [10.0, 10.0],
            blocks: blocks.to_vec(),
        });
        let editor = BlockEditor {
            gesture: Gesture::Dragging(Drag {
                fragment: program.run_at(second).unwrap(),
                source: DragSource::Canvas { head: second },
                grab_offset: Vec2::ZERO,
                head: pos2(400.0, 300.0),
                snap: None,
            }),
            ..BlockEditor::default()
        };
        (language, program, editor)
    }

    #[test]
    fn a_run_in_hand_stays_in_the_program_until_dropped() {
        let (language, mut program, mut editor) = dragging_the_tail();
        assert_eq!(program.stacks[0].blocks.len(), 3);
        assert_eq!(editor.lifted(), Some(program.stacks[0].blocks[1].id));

        let Gesture::Dragging(drag) = std::mem::take(&mut editor.gesture) else {
            unreachable!()
        };
        assert!(drop_run(&language, &mut program, drag, false));
        let lengths: Vec<_> = program.stacks.iter().map(|stack| stack.blocks.len()).collect();
        assert_eq!(lengths, [1, 2]);
        assert_eq!(program.stacks[1].pos, [400.0, 300.0]);
    }

    #[test]
    fn canceling_a_drag_leaves_the_program_as_it_was() {
        let (_, program, mut editor) = dragging_the_tail();
        let before = program.stacks.clone();
        assert!(editor.cancel_drag());
        assert!(!editor.is_dragging());
        assert!(!editor.cancel_drag(), "nothing left to cancel");
        assert_eq!(program.stacks, before);
    }

    #[test]
    fn a_run_dropped_into_another_program_is_let_go() {
        let (language, _, mut editor) = dragging_the_tail();
        let (_, mut other, _) = dragging_the_tail();
        let second = other.stacks[0].blocks[1].id;
        other.set_literal(second, "value", "another creature".into());
        let before = other.stacks.clone();

        let Gesture::Dragging(drag) = std::mem::take(&mut editor.gesture) else {
            unreachable!()
        };
        assert!(!drop_run(&language, &mut other, drag, false));
        assert_eq!(other.stacks, before);
    }

    #[test]
    fn a_run_dropped_into_an_identical_program_lands_there() {
        let (language, _, mut editor) = dragging_the_tail();
        let (_, mut copy, _) = dragging_the_tail();

        let Gesture::Dragging(drag) = std::mem::take(&mut editor.gesture) else {
            unreachable!()
        };
        assert!(drop_run(&language, &mut copy, drag, false));
        let lengths: Vec<_> = copy.stacks.iter().map(|stack| stack.blocks.len()).collect();
        assert_eq!(lengths, [1, 2]);
    }

    #[test]
    fn going_read_only_mid_drag_lets_the_run_go_without_dropping_it() {
        let (language, mut program, mut editor) = dragging_the_tail();
        let before = program.stacks.clone();
        editor.options.read_only = true;

        // No pointer is down, so this frame would otherwise release the run.
        let mut changed = true;
        let mut frame = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            changed = editor.show(ui, &language, &mut program).changed;
        });
        frame.textures_delta.clear();
        assert!(!changed);
        assert!(!editor.is_dragging());
        assert_eq!(program.stacks, before);
    }

    /// A "codes" language whose one literal normalizes to upper case, holding
    /// "ab" in a field the editor is editing.
    fn editing_a_code() -> (Language, Program, BlockId, BlockEditor) {
        #[derive(Debug)]
        struct Upper;
        impl block_parse::LiteralValidator for Upper {
            fn validate(&self, text: &str) -> Result<block_parse::Value, String> {
                Ok(block_parse::Value::Text(text.into()))
            }
            fn normalize(&self, text: &str) -> String {
                text.to_ascii_uppercase()
            }
        }
        let mut validators = block_parse::Validators::new();
        validators.insert("upper", std::sync::Arc::new(Upper));
        let language = Language::from_ron(
            r#"Language(
                name: "codes",
                file: (extension: "c"),
                types: { "code": (literal: Custom("upper")) },
                blocks: [(id: "start", name: "Start", spec: "start {code:code}")],
            )"#,
            &validators,
        )
        .unwrap();
        let mut program = Program::new(&language);
        let start = program.instantiate(&language, "start").unwrap();
        let id = start.id;
        program.stacks.push(block_parse::Stack {
            pos: [0.0, 0.0],
            blocks: vec![start],
        });
        program.set_literal(id, "code", "ab".into());

        let editor = BlockEditor {
            edit: Some(LiteralEdit {
                block: id,
                input: "code".into(),
            }),
            ..BlockEditor::default()
        };
        (language, program, id, editor)
    }

    #[test]
    fn a_field_left_without_being_drawn_is_still_normalized() {
        let (language, mut program, id, mut editor) = editing_a_code();
        // A fresh context has nothing focused, as after the field went away.
        assert!(editor.settle_edit(&egui::Context::default(), &language, &mut program));
        assert_eq!(program.find(id).unwrap().inputs["code"].literal.as_deref(), Some("AB"));
        assert!(editor.edit.is_none());
    }

    #[test]
    fn committing_normalizes_a_field_that_still_has_focus() {
        let (language, mut program, id, mut editor) = editing_a_code();
        let ctx = egui::Context::default();
        let field = editor.field_id(id, "code");
        ctx.memory_mut(|memory| memory.request_focus(field));

        assert!(!editor.settle_edit(&ctx, &language, &mut program), "still being typed into");
        assert!(editor.commit_edit(&ctx, &language, &mut program));
        assert_eq!(program.find(id).unwrap().inputs["code"].literal.as_deref(), Some("AB"));
        assert!(!ctx.memory(|memory| memory.has_focus(field)));
        assert!(editor.edit.is_none());
    }

    /// One frame of `editor` over `program` with `events`, on a screen whose
    /// canvas starts at the left edge. Returns every piece of text painted.
    fn frame(
        ctx: &egui::Context,
        editor: &mut BlockEditor,
        language: &Language,
        program: &mut Program,
        overlay: &Overlay,
        events: Vec<egui::Event>,
    ) -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            editor.show_with(ui, language, program, overlay);
        });
        out.textures_delta.clear();

        fn walk(shape: &egui::Shape, into: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, into)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
        texts
    }

    /// Presses at `at` and drags well past the threshold, holding on.
    fn press_and_drag(
        ctx: &egui::Context,
        editor: &mut BlockEditor,
        language: &Language,
        program: &mut Program,
        overlay: &Overlay,
        at: Pos2,
    ) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let away = at + vec2(60.0, 40.0);
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![egui::Event::PointerMoved(away)],
            vec![egui::Event::PointerMoved(away + vec2(5.0, 0.0))],
        ] {
            frame(ctx, editor, language, program, overlay, events);
        }
    }

    /// A language of one switchable block, a program holding it at the canvas
    /// origin, a context with fonts, and the screen points of its switch and
    /// of its label, for an editor made by [`codon_editor`].
    fn codon_on_canvas() -> (Language, Program, egui::Context, Pos2, Pos2) {
        let language = Language::from_ron(
            r#"Language(
                name: "codons",
                file: (extension: "c"),
                blocks: [(id: "start", name: "Start", spec: "start codon", switch: true)],
            )"#,
            &block_parse::Validators::new(),
        )
        .unwrap();
        let mut program = Program::new(&language);
        let start = program.instantiate(&language, "start").unwrap();
        program.stacks.push(block_parse::Stack {
            pos: [0.0, 0.0],
            blocks: vec![start],
        });

        let ctx = egui::Context::default();
        let mut editor = codon_editor();
        // Once, so fonts exist to lay the block out with.
        frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![]);
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let scene = Layout {
            language: &language,
            measure: &EguiMeasure(&ctx),
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
        }
        .program(&program);
        let placed = &scene.blocks[0];
        let to_screen = |point: Pos2| point + editor.view.pan;
        let switch = to_screen(placed.switch.expect("the language gives it a switch").center());
        let label = to_screen(placed.rect.left_center() + vec2(8.0, 0.0));
        (language, program, ctx, switch, label)
    }

    fn codon_editor() -> BlockEditor {
        let mut editor = BlockEditor::default();
        editor.options.palette_width = Some(0.0);
        editor
    }

    #[test]
    fn a_switch_without_state_is_not_a_handle_on_its_block() {
        let (language, mut program, ctx, switch, label) = codon_on_canvas();
        let no_state = Overlay::default();
        let mut editor = codon_editor();
        press_and_drag(&ctx, &mut editor, &language, &mut program, &no_state, switch);
        assert!(!editor.is_dragging(), "a press on the switch picked the block up");

        let mut editor = codon_editor();
        press_and_drag(&ctx, &mut editor, &language, &mut program, &no_state, label);
        assert!(editor.is_dragging(), "the same gesture on the label should drag");
    }

    #[test]
    fn a_disabled_switch_says_why_when_hovered() {
        let (language, mut program, ctx, switch, label) = codon_on_canvas();
        let codon = program.stacks[0].blocks[0].id;
        ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
        let hint = "select a cell first";
        let hinted = Overlay {
            switch_hint: Some(hint.into()),
            ..Overlay::default()
        };
        let mut hover = |at: Pos2, overlay: &Overlay| {
            let mut editor = codon_editor();
            let mut texts = Vec::new();
            for events in [vec![egui::Event::PointerMoved(at)], vec![], vec![]] {
                texts = frame(&ctx, &mut editor, &language, &mut program, overlay, events);
            }
            texts.iter().any(|text| text == hint)
        };

        assert!(hover(switch, &hinted), "no tooltip over the disabled switch");
        assert!(!hover(label, &hinted), "only the switch carries it");

        let mut live = hinted.clone();
        live.switches.insert(codon, true);
        assert!(!hover(switch, &live), "a live switch needs no excuse");
    }

    /// A block with a choice of "red", "green" and "blue" at the canvas origin,
    /// a context with fonts, and the screen rect of its field.
    fn paint_on_canvas() -> (Language, Program, egui::Context, Rect) {
        let language = Language::from_ron(
            r#"Language(
                name: "paints",
                file: (extension: "p"),
                types: { "hue": (literal: Choice(["red", "green", "blue"])) },
                blocks: [(id: "paint", name: "Paint", spec: "paint {hue:hue}")],
            )"#,
            &block_parse::Validators::new(),
        )
        .unwrap();
        let mut program = Program::new(&language);
        let paint = program.instantiate(&language, "paint").unwrap();
        program.stacks.push(block_parse::Stack {
            pos: [0.0, 0.0],
            blocks: vec![paint],
        });

        let ctx = egui::Context::default();
        let mut editor = codon_editor();
        frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![]);
        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let scene = Layout {
            language: &language,
            measure: &EguiMeasure(&ctx),
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
        }
        .program(&program);
        let field = scene.slots().next().unwrap().rect.translate(editor.view.pan);
        (language, program, ctx, field)
    }

    fn click(at: Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]]
    }

    #[test]
    fn a_choice_opens_its_own_menu_and_sets_what_is_picked() {
        let (language, mut program, ctx, field) = paint_on_canvas();
        let paint = program.stacks[0].blocks[0].id;
        let hue = |program: &Program| program.find(paint).unwrap().inputs["hue"].literal.clone();
        let mut editor = codon_editor();
        let mut texts = Vec::new();
        for events in click(field.center()) {
            texts = frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(editor.choice.as_ref().is_some_and(|open| open.is(paint, "hue")));
        for option in ["green", "blue"] {
            assert!(texts.iter().any(|text| text == option), "{option} is not shown: {texts:?}");
        }
        assert!(!editor.is_dragging());

        // Room below, so the menu hangs there; its third row is "blue".
        let body_top = field.max.y + dropdown::POINTER + dropdown::GAP;
        let blue = pos2(field.center().x, body_top + dropdown::INSET + 2.5 * dropdown::ROW_HEIGHT);
        for events in click(blue) {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert_eq!(hue(&program).as_deref(), Some("blue"));
        assert!(editor.choice.is_none(), "picking closes the menu");

        for events in click(field.center()) {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(editor.choice.as_ref().is_some_and(|open| open.is(paint, "hue")), "reopened");
        let escape = egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![escape]);
        assert!(editor.choice.is_none(), "Escape closes the menu");

        for events in click(field.center()) {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(editor.choice.as_ref().is_some_and(|open| open.is(paint, "hue")), "reopened");
        for events in click(pos2(700.0, 500.0)) {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(editor.choice.is_none(), "a press elsewhere closes the menu");
        assert_eq!(hue(&program).as_deref(), Some("blue"));
    }

    #[test]
    fn the_editor_and_what_it_edits_can_live_in_a_bevy_resource() {
        send_and_sync::<BlockEditor>();
        send_and_sync::<Language>();
        send_and_sync::<Program>();
    }
}
