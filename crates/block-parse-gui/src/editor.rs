use block_parse::ast::Script;
use block_parse::host::{Overlay, RunCommand, TabId};
use block_parse::edit::{Fragment, Target};
use block_parse::language::LiteralKind;
use block_parse::program::{BlockId, Program, Slot};
use block_parse::Language;
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Frame, Key, LayerId, Margin, Modifiers, Order, Pos2,
    Rect, Sense, TextEdit, UiBuilder, Vec2, pos2, vec2,
};

use crate::color::{SwatchRecipe, Swatches};

type SwatchKey = (
    SwatchRecipe,
    Vec<block_parse::CategoryColor>,
    Vec<Option<block_parse::CategoryColor>>,
);
use crate::dropdown::Menu;
use crate::interact::{
    DRAG_THRESHOLD, Drag, DragSource, Gesture, LiteralEdit, OpenChoice, Press, Pressed, drop_run, find_snap,
};
use crate::layout::{
    FAINT_SIZE, Font, LABEL_SIZE, LITERAL_SIZE, Layout, Measure, PlacedBlock, PlacedSlot, Scene, SlotContent,
};
use crate::paint::{self, Transform};
use crate::panels::Panels;
use crate::tabs::{self, TabState};
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
    /// A first click awaiting its second; any other press clears it. egui's
    /// own double-click becomes a triple when it follows another closely.
    last_click: Option<(BlockId, f64)>,
    tabs: TabState,
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
    /// `None` fits the widest block. Dragging the edge sets it, for the host
    /// to keep.
    pub palette_width: Option<f32>,
    /// Hides the palette without forgetting `palette_width`.
    pub palette_collapsed: bool,
    /// The panel right of the canvas, holding the host's `Overlay::tabs`.
    pub side_width: f32,
    pub side_collapsed: bool,
    /// The side panel's tabs left to right. The editor keeps it in step with
    /// the host's tabs: new ones open after the active tab.
    pub tab_order: Vec<TabId>,
    /// Set it to bring a tab to the front, even one the host adds this frame.
    pub active_tab: Option<TabId>,
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
            palette_collapsed: false,
            side_width: 320.0,
            side_collapsed: true,
            tab_order: Vec::new(),
            active_tab: None,
            theme: Theme::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditorOutput {
    /// Any AST the consumer holds is stale.
    pub changed: bool,
    /// Record the program in a [`History`](block_parse::History) now. Held
    /// back while a literal is typed, so the entry is one step.
    pub settled: bool,
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
    /// Double-clicked: a request to run `script`, [`Program::script_at`] the
    /// block. Sent in read-only mode too. Anything the host has to say back
    /// goes in its `Overlay::bubbles`.
    Run { block: BlockId, script: Script },
    /// The user asked for a block's `documentation`. The editor never opens
    /// links itself; this is the consumer's hook to open, resolve or refuse.
    OpenDocumentation { opcode: String, link: String },
    /// A request to show `script`, [`Program::script_at`] the block, as the
    /// host's text, typically in a tab of its `Overlay::tabs`. Sent in
    /// read-only mode too.
    Inspect { block: BlockId, script: Script },
    /// A request; the tab stays until the host stops sending it.
    CloseTab(TabId),
    /// Enter pressed on a console tab's line, sent without its newline.
    ConsoleInput { tab: TabId, line: String },
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
    time: f64,
    double_click_delay: f64,
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
            last_click: None,
            tabs: TabState::default(),
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
        let was_editing = self.edit.is_some();
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
        let panels = Panels::new(bounds, &self.options, palette.width);
        tabs::arrange(&mut self.options, &mut self.tabs, &overlay.tabs);
        let (palette_rect, canvas_rect) = (panels.palette, panels.canvas);

        let double_click_delay = ctx.options(|o| o.input_options.max_double_click_delay);
        let input = ctx.input(|i| PointerInput {
            at: i.pointer.hover_pos(),
            pressed: i.pointer.primary_pressed(),
            time: i.time,
            double_click_delay,
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
        let divider = over.and_then(|at| panels.edge_at(&self.options, at));
        // The side panel's widgets take what lands on them.
        let over = over.filter(|at| divider.is_some() || !panels.side.contains(*at));
        let on_toggle = over.is_some_and(|at| panels.on_toggle(at));

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
            editing: edit.as_ref().map(|edit| (edit.block, &edit.slot)),
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
            self.last_click = None;
            self.gesture = self.start_drag(press, language, program, t);
        }
        // A drop now would edit a program the host has locked.
        if read_only && self.is_dragging() {
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
                .is_some_and(|slot| open.is(slot.parent, &slot.slot));
            if !on_menu && !on_field {
                self.choice = None;
            }
        }

        match std::mem::take(&mut self.gesture) {
            Gesture::Idle => {
                if let Some((at, edge)) = over.zip(divider).filter(|_| input.pressed) {
                    let grab = panels.grab(edge, at);
                    self.gesture = Gesture::Resizing { edge, grab };
                    self.last_click = None;
                } else if let Some(at) = over.filter(|_| input.pressed && !on_toggle) {
                    self.gesture = self.press(at, &scene, &palette, palette_rect, palette_t, t, overlay);
                    // Only an uninterrupted pair of clicks on one block runs it.
                    let same = match &self.gesture {
                        Gesture::Pressed(Press {
                            on: Pressed::Block { id, .. },
                            ..
                        }) => self.last_click.is_some_and(|(last, _)| last == *id),
                        _ => false,
                    };
                    if !same {
                        self.last_click = None;
                    }
                }
            }
            Gesture::Pressed(press) => {
                if input.down {
                    self.gesture = Gesture::Pressed(press);
                } else if let Pressed::Block { id, .. } = press.on {
                    output.events.push(EditorEvent::BlockClicked(id));
                    let second = self
                        .last_click
                        .take()
                        .is_some_and(|(last, at)| last == id && input.time - at <= input.double_click_delay);
                    if !second {
                        self.last_click = Some((id, input.time));
                    } else if let Some(script) = program.script_at(language, id) {
                        output.events.push(EditorEvent::Run { block: id, script });
                    }
                }
            }
            Gesture::Panning => {
                if input.down {
                    self.view.pan += input.delta;
                    self.gesture = Gesture::Panning;
                }
            }
            Gesture::Resizing { edge, grab } => {
                if let Some(at) = input.at {
                    panels.resize(&mut self.options, edge, at.x - grab);
                }
                if input.down {
                    self.gesture = Gesture::Resizing { edge, grab };
                }
            }
            Gesture::Dragging(mut drag) => {
                if let Some(at) = input.at {
                    drag.head = t.canvas(at) - drag.grab_offset;
                }
                // A drop under the side panel could not be seen, so it is canceled.
                let hidden = input.at.is_some_and(|at| panels.side.contains(at));
                // Every frame, release included, so a quick flick still snaps.
                let run = layout.run(&drag.fragment.blocks, drag.head);
                drag.snap = (!hidden)
                    .then(|| find_snap(language, program, &scene, &drag.fragment, &run))
                    .flatten();
                if input.down {
                    self.gesture = Gesture::Dragging(drag);
                } else if !hidden {
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
        // Fields share the canvas's layer, which paints in call order: each
        // block's widgets go in right after it, under every later block.
        let mut fields = ui.new_child(UiBuilder::new().max_rect(canvas_rect));
        fields.set_clip_rect(canvas_rect);
        paint::scene_with(&canvas, &scene, t, &theme, !read_only, overlay, |block| {
            self.block_widgets(&mut fields, block, canvas_rect, t, &theme, overlay, language, program, &mut output);
        });
        if let Gesture::Dragging(drag) = &self.gesture
            && let Some((_, mark)) = &drag.snap
        {
            paint::snap_mark(&canvas, mark, t, &theme);
        }
        let hot = match self.gesture {
            Gesture::Resizing { edge, .. } => Some(edge),
            Gesture::Idle => divider,
            _ => None,
        };
        // Over the fields on this layer; under the menu and run in hand on theirs.
        let visible = Rect::from_min_max(t.canvas(canvas_rect.min), t.canvas(canvas_rect.max));
        let bubbles = paint::place_bubbles(fields.painter(), &scene, t.zoom, &theme, overlay, visible);
        paint::bubbles(fields.painter(), bubbles, t, &theme);
        if panels.toggles(ui, &mut self.options) {
            ctx.request_repaint();
        }
        tabs::show(ui, panels.side, self.id, &mut self.options, &mut self.tabs, &overlay.tabs, &theme, &mut output.events);
        // After the side panel, whose fill would cover half the line.
        panels.edges(&ui.painter_at(bounds), hot, &theme);
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
        } else if hot.is_some() {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        } else if on_toggle {
        } else if let Some(at) = over {
            let on_palette = palette_rect.contains(at);
            let field = scene
                .slot_at(t.canvas(at))
                .filter(|slot| !on_palette && !read_only && slot.is_field())
                .and_then(|slot| match &slot.content {
                    SlotContent::Literal { kind, .. } | SlotContent::Append { kind } => Some(kind),
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

        output.settled = self.edit.is_none() && (output.changed || was_editing);
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
                .is_some_and(PlacedSlot::is_field);
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
            if ui.button("Delete").clicked() {
                program.remove(id);
                output.changed = true;
                self.menu = None;
                ui.close();
                return;
            }
            ui.separator();
        }
        if self.options.breakpoints && ui.button("Toggle breakpoint").clicked() {
            output.events.push(EditorEvent::ToggleBreakpoint(id));
            ui.close();
        }
        if ui.button("Inspect").clicked() {
            if let Some(script) = program.script_at(language, id) {
                output.events.push(EditorEvent::Inspect { block: id, script });
            }
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
    /// starts, so nothing depends on draw order, and before undoing, so the
    /// edit is one step. Record the program after. True if the program changed.
    pub fn commit_edit(&mut self, ctx: &egui::Context, language: &Language, program: &mut Program) -> bool {
        if let Some(edit) = &self.edit {
            let id = self.field_id(edit.block, &edit.slot);
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
            .find(|slot| open.is(slot.parent, &slot.slot))
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
            swatch.highlight = swatch.muted_highlight;
            swatch.shadow = swatch.muted_shadow;
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
        options[index] != *text && program.set_literal(slot.parent, &slot.slot, options[index].clone())
    }

    /// A list's empty slot and the item typing into it creates share an id,
    /// so focus survives the append.
    fn field_id(&self, block: BlockId, slot: &Slot) -> egui::Id {
        self.id.with((block, slot))
    }

    /// Normalizes a literal that lost focus without its field being drawn,
    /// such as one panned off the canvas or left when the editor went
    /// read-only. True if the program changed.
    fn settle_edit(&mut self, ctx: &egui::Context, language: &Language, program: &mut Program) -> bool {
        let Some(edit) = self.edit.clone() else {
            return false;
        };
        let id = self.field_id(edit.block, &edit.slot);
        if ctx.memory(|memory| memory.has_focus(id)) {
            return false;
        }
        self.edit = None;
        let Some(block) = program.find(edit.block) else {
            return false;
        };
        let ty = language
            .block(&block.opcode)
            .and_then(|def| def.slot_type(&edit.slot))
            .map(str::to_owned);
        let text = block.slot(&edit.slot).and_then(|input| input.literal.clone());
        let (Some(ty), Some(text)) = (ty, text) else {
            return false;
        };
        let tidied = language.normalize_literal(&ty, &text);
        tidied != text && program.set_literal(edit.block, &edit.slot, tidied)
    }

    /// A block's switch and fields.
    #[allow(clippy::too_many_arguments)]
    fn block_widgets(
        &mut self,
        ui: &mut egui::Ui,
        block: &PlacedBlock,
        canvas_rect: Rect,
        t: Transform,
        theme: &Theme,
        overlay: &Overlay,
        language: &Language,
        program: &mut Program,
        output: &mut EditorOutput,
    ) {
        // Live even when read-only: switches are the host's, not program data.
        if let Some(rect) = block.switch.map(|rect| t.rect(rect))
            && !block.switch_covered
            && canvas_rect.intersects(rect)
        {
            match overlay.switches.get(&block.id) {
                Some(&on) => {
                    let mut value = on;
                    if checkbox(ui, rect.center(), t.zoom, &mut value).changed() {
                        output.events.push(EditorEvent::Switched(block.id, value));
                    }
                }
                // Hover only, so a press still reaches `press`, which ignores it.
                None => {
                    if let Some(hint) = &overlay.switch_hint {
                        ui.interact(rect, self.id.with(("switch", block.id)), Sense::hover()).on_hover_text(hint);
                    }
                }
            }
        }
        if self.options.read_only {
            return;
        }
        for slot in block.slots.iter().filter(|slot| slot.is_field()) {
            let field = match &slot.content {
                SlotContent::Literal { kind, text, .. } => Some((kind, text.as_str())),
                SlotContent::Append { kind } => Some((kind, "")),
                SlotContent::Empty | SlotContent::Plugged(_) => None,
            };
            if let Some((kind, text)) = field {
                let rect = t.rect(slot.rect);
                if canvas_rect.intersects(rect)
                    && self.literal_field(ui, rect, slot, kind, text, language, program, t.zoom, theme)
                {
                    output.changed = true;
                }
            }
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
        let id = self.field_id(slot.parent, &slot.slot);
        match kind {
            LiteralKind::Bool => {
                let mut on = text == "true";
                if checkbox(ui, rect.center(), zoom, &mut on).changed() {
                    return program.set_literal(slot.parent, &slot.slot, on.to_string());
                }
                false
            }
            LiteralKind::Choice(_) => {
                if ui.interact(rect, id, Sense::click()).clicked() {
                    let open = self.choice.as_ref().is_some_and(|open| open.is(slot.parent, &slot.slot));
                    self.choice = (!open).then(|| OpenChoice {
                        block: slot.parent,
                        slot: slot.slot.clone(),
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
                let this = |edit: &LiteralEdit| edit.block == slot.parent && edit.slot == slot.slot;
                // egui reports a loss for two frames; once settled, a second
                // pass would normalize whatever an undo just put back.
                let editing = self.edit.as_ref().is_some_and(this);
                if response.has_focus() {
                    self.edit = Some(LiteralEdit {
                        block: slot.parent,
                        slot: slot.slot.clone(),
                    });
                } else if editing {
                    self.edit = None;
                }
                if response.lost_focus() && editing {
                    let tidied = language.normalize_literal(&slot.ty, &buffer);
                    return tidied != text && program.set_literal(slot.parent, &slot.slot, tidied);
                }
                response.changed() && program.set_literal(slot.parent, &slot.slot, buffer)
            }
        }
    }
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
        .find(|block| !block.switch_covered && block.switch.is_some_and(|rect| rect.contains(point)))
        .map(|block| (block.id, overlay.switches.contains_key(&block.id)))
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

pub(crate) struct EguiMeasure<'a>(pub &'a egui::Context);

impl Measure for EguiMeasure<'_> {
    fn text_width(&self, text: &str, font: Font) -> f32 {
        let size = match font {
            Font::Label => LABEL_SIZE,
            Font::Faint => FAINT_SIZE,
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
    use block_parse::host::{Tab, TabContent};
    use crate::panels::{MIN_CANVAS_WIDTH, TOGGLE_SIZE};
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
        other.set_literal(second, &Slot::input("value"), "another creature".into());
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
        program.set_literal(id, &Slot::input("code"), "ab".into());

        let editor = BlockEditor {
            edit: Some(LiteralEdit {
                block: id,
                slot: Slot::input("code"),
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
        let field = editor.field_id(id, &Slot::input("code"));
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

    #[test]
    fn a_stack_on_top_hides_and_takes_presses_for_the_fields_under_it() {
        let language = tiny();
        let mut program = Program::new(&language);
        let mut ids = Vec::new();
        for value in ["under", "over"] {
            let mut print = program.instantiate(&language, "print").unwrap();
            print.inputs.get_mut("value").unwrap().literal = Some(value.into());
            ids.push(print.id);
            program.stacks.push(block_parse::Stack {
                pos: [0.0, 0.0],
                blocks: vec![print],
            });
        }
        let ctx = egui::Context::default();
        let mut editor = codon_editor();
        frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![]);
        let texts = frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![]);
        let at = |text: &str| texts.iter().position(|painted| painted == text).unwrap();
        let top_label = texts.iter().rposition(|painted| painted == "print").unwrap();
        assert!(at("under") < top_label, "the lower field painted over the top block: {texts:?}");
        assert!(at("over") > top_label);

        let swatches = Swatches::resolve(&language, &SwatchRecipe::default());
        let layout = Layout {
            language: &language,
            measure: &EguiMeasure(&ctx),
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
        };
        // The top block's edge just inside the lower field, so its own field is clear.
        let field = layout.program(&program).blocks[0].slots[0].rect;
        program.stacks[1].pos = [field.min.x - 4.0, 0.0];
        assert!(layout.program(&program).blocks[0].slots[0].covered);
        let at = field.left_center() + vec2(2.0, 0.0) + editor.view.pan;
        press_and_drag(&ctx, &mut editor, &language, &mut program, &Overlay::default(), at);
        assert_eq!(editor.lifted(), Some(ids[1]));
    }

    #[test]
    fn a_stack_on_top_covers_the_error_tags_under_it() {
        let language = tiny();
        let mut program = Program::new(&language);
        let mut add = program.instantiate(&language, "add").unwrap();
        add.inputs.get_mut("a").unwrap().literal = Some("abc".into());
        let print = program.instantiate(&language, "print").unwrap();
        for block in [add, print] {
            program.stacks.push(block_parse::Stack {
                pos: [0.0, 0.0],
                blocks: vec![block],
            });
        }
        let ctx = egui::Context::default();
        let mut editor = codon_editor();
        frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![]);
        let texts = frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![]);
        let tag = texts.iter().position(|painted| painted.contains("number")).unwrap();
        let label = texts.iter().rposition(|painted| painted == "print").unwrap();
        assert!(tag < label, "the tag painted over the stack on top: {texts:?}");
    }

    fn codon_editor() -> BlockEditor {
        let mut editor = BlockEditor::default();
        editor.options.palette_width = Some(0.0);
        editor
    }

    #[test]
    fn dragging_the_palette_edge_resizes_it() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut editor = BlockEditor::default();
        editor.options.palette_width = Some(200.0);
        let edge = pos2(201.0, 300.0);
        press_and_drag(&ctx, &mut editor, &language, &mut program, &Overlay::default(), edge);
        assert!(!editor.is_dragging());
        assert_eq!(editor.options.palette_width, Some(265.0));

        let far = egui::Event::PointerMoved(pos2(1190.0, 300.0));
        frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), vec![far]);
        assert_eq!(editor.options.palette_width, Some(1200.0 - MIN_CANVAS_WIDTH));
    }

    #[test]
    fn the_palette_collapses_and_comes_back_at_its_width() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut editor = BlockEditor::default();
        editor.options.palette_width = Some(200.0);
        let mut click = |editor: &mut BlockEditor, at: Pos2| {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
                frame(&ctx, editor, &language, &mut program, &Overlay::default(), events);
            }
        };
        let toggle = |width: f32| pos2(width + TOGGLE_SIZE.x, TOGGLE_SIZE.x / 2.0 + TOGGLE_SIZE.y / 2.0);

        click(&mut editor, toggle(200.0));
        assert!(editor.options.palette_collapsed);
        assert!(!editor.is_dragging());
        click(&mut editor, toggle(0.0));
        assert!(!editor.options.palette_collapsed);
        assert_eq!(editor.options.palette_width, Some(200.0));
    }

    #[test]
    fn the_side_panel_keeps_presses_and_drops_from_the_canvas() {
        let (language, mut program, ctx, _, label) = codon_on_canvas();
        let mut editor = codon_editor();
        editor.options.side_collapsed = false;
        let pan = editor.view.pan;
        press_and_drag(&ctx, &mut editor, &language, &mut program, &Overlay::default(), pos2(1100.0, 300.0));
        assert_eq!(editor.view.pan, pan, "a drag over the side panel panned the canvas");

        let before = program.clone();
        press_and_drag(&ctx, &mut editor, &language, &mut program, &Overlay::default(), label);
        assert!(editor.is_dragging());
        let over = pos2(1100.0, 300.0);
        let release = egui::Event::PointerButton {
            pos: over,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        };
        for events in [vec![egui::Event::PointerMoved(over)], vec![release]] {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(!editor.is_dragging());
        assert_eq!(program.stacks, before.stacks, "a run dropped on the side panel should stay where it was");
    }

    #[test]
    fn the_side_panel_opens_and_its_edge_drags() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut editor = BlockEditor::default();
        assert!(editor.options.side_collapsed);
        let at = pos2(1200.0 - TOGGLE_SIZE.x, TOGGLE_SIZE.x / 2.0 + TOGGLE_SIZE.y / 2.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(!editor.options.side_collapsed);

        let edge = pos2(1200.0 - 320.0 + 1.0, 300.0);
        press_and_drag(&ctx, &mut editor, &language, &mut program, &Overlay::default(), edge);
        assert!(!editor.is_dragging());
        assert_eq!(editor.options.side_width, 320.0 - 65.0);
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

    /// What a fresh read-only editor sends for clicks at `(point, time)`.
    fn clicks(
        ctx: &egui::Context,
        language: &Language,
        program: &mut Program,
        clicks: &[(Pos2, f64)],
    ) -> Vec<EditorEvent> {
        let mut editor = codon_editor();
        editor.options.read_only = true;
        let mut events = Vec::new();
        for &(at, time) in clicks {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            for input in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
                let raw = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
                    time: Some(time),
                    events: input,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    events.extend(editor.show_with(ui, language, program, &Overlay::default()).events);
                });
                out.textures_delta.clear();
            }
        }
        events
    }

    fn runs(events: &[EditorEvent]) -> Vec<(BlockId, usize)> {
        events
            .iter()
            .filter_map(|event| match event {
                EditorEvent::Run { block, script } => Some((*block, script.body.len())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_second_quick_click_on_the_same_block_runs_it_and_nothing_else_does() {
        let (language, mut program, ctx, _, label) = codon_on_canvas();
        let codon = program.stacks[0].blocks[0].id;
        let other = program.instantiate(&language, "start").unwrap();
        program.stacks.push(block_parse::Stack {
            pos: [0.0, 100.0],
            blocks: vec![other],
        });
        let below = label + vec2(0.0, 100.0);
        let empty = label + vec2(400.0, 300.0);
        // One context throughout, whose clock cannot go back.
        let mut start = 0.0;
        let mut run = |sequence: &[(Pos2, f64)]| {
            start += 10.0;
            let sequence: Vec<_> = sequence.iter().map(|&(at, time)| (at, start + time)).collect();
            runs(&clicks(&ctx, &language, &mut program, &sequence))
        };

        assert_eq!(run(&[(label, 1.0)]), vec![]);
        assert_eq!(run(&[(label, 1.0), (label, 1.1)]), vec![(codon, 1)]);
        assert_eq!(run(&[(label, 1.0), (label, 2.0)]), vec![], "too slow");
        assert_eq!(run(&[(label, 1.0), (below, 1.1)]), vec![], "two blocks");
        assert_eq!(run(&[(label, 1.0), (empty, 1.05), (label, 1.1)]), vec![], "interrupted");
        assert_eq!(
            run(&[(label, 1.0), (label, 1.1), (label, 1.2)]),
            vec![(codon, 1)],
            "a third click starts a new pair"
        );
    }

    #[test]
    fn a_read_only_canvas_still_pans() {
        let (language, mut program, ctx, _, label) = codon_on_canvas();
        let mut editor = codon_editor();
        editor.options.read_only = true;
        let before = editor.view.pan;
        press_and_drag(&ctx, &mut editor, &language, &mut program, &Overlay::default(), label + vec2(400.0, 300.0));
        assert_ne!(editor.view.pan, before);
    }

    #[test]
    fn the_hosts_bubbles_are_drawn() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut overlay = Overlay::default();
        overlay.bubbles.insert(program.stacks[0].blocks[0].id, "ran fine".into());
        let mut editor = codon_editor();
        let texts = frame(&ctx, &mut editor, &language, &mut program, &overlay, vec![]);
        assert!(texts.iter().any(|text| text == "ran fine"), "{texts:?}");
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
    fn typing_into_a_lists_empty_slot_appends_without_losing_focus() {
        let language = Language::from_ron(
            r#"Language(
                name: "sums",
                file: (extension: "s"),
                types: { "number": (literal: Number) },
                blocks: [(id: "sum", name: "Sum", kind: Reporter("number"), spec: "sum {xs:number*}")],
            )"#,
            &block_parse::Validators::new(),
        )
        .unwrap();
        let mut program = Program::new(&language);
        let sum = program.instantiate(&language, "sum").unwrap();
        let id = sum.id;
        program.stacks.push(block_parse::Stack {
            pos: [0.0, 0.0],
            blocks: vec![sum],
        });
        let ctx = egui::Context::default();
        let mut editor = codon_editor();
        let overlay = Overlay::default();
        frame(&ctx, &mut editor, &language, &mut program, &overlay, vec![]);
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
        let empty = scene.slots().next().unwrap().rect.translate(editor.view.pan);
        let items = |program: &Program| -> Vec<Option<String>> {
            let block = program.find(id).unwrap();
            block.lists.get("xs").into_iter().flatten().map(|item| item.literal.clone()).collect()
        };

        for events in click(empty.center()) {
            frame(&ctx, &mut editor, &language, &mut program, &overlay, events);
        }
        for key in ["4", "2"] {
            frame(&ctx, &mut editor, &language, &mut program, &overlay, vec![egui::Event::Text(key.into())]);
        }
        assert_eq!(items(&program), [Some("42".to_owned())]);
        let typing = Some(LiteralEdit {
            block: id,
            slot: Slot::item("xs", 0),
        });
        assert_eq!(editor.edit, typing, "the new item has the focus the empty slot had");

        let backspace = egui::Event::Key {
            key: Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        for _ in 0..2 {
            frame(&ctx, &mut editor, &language, &mut program, &overlay, vec![backspace.clone()]);
        }
        assert!(items(&program).is_empty(), "emptied, the last item goes");
        assert_eq!(editor.edit, typing, "and the field is the empty slot again");
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
        assert!(editor.choice.as_ref().is_some_and(|open| open.is(paint, &Slot::input("hue"))));
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
        assert!(editor.choice.as_ref().is_some_and(|open| open.is(paint, &Slot::input("hue"))), "reopened");
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
        assert!(editor.choice.as_ref().is_some_and(|open| open.is(paint, &Slot::input("hue"))), "reopened");
        for events in click(pos2(700.0, 500.0)) {
            frame(&ctx, &mut editor, &language, &mut program, &Overlay::default(), events);
        }
        assert!(editor.choice.is_none(), "a press elsewhere closes the menu");
        assert_eq!(hue(&program).as_deref(), Some("blue"));
    }

    /// [`editing_a_code`] on a canvas with nothing focused yet, and the screen
    /// rect of its field.
    fn a_code_on_canvas() -> (Language, Program, BlockId, egui::Context, BlockEditor, Rect) {
        let (language, mut program, id, _) = editing_a_code();
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
        (language, program, id, ctx, editor, field)
    }

    fn recorded(
        ctx: &egui::Context,
        editor: &mut BlockEditor,
        language: &Language,
        program: &mut Program,
        history: &mut block_parse::History,
        events: Vec<egui::Event>,
    ) -> EditorOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut output = EditorOutput::default();
        ctx.run_ui(input, |ui| output = editor.show_with(ui, language, program, &Overlay::default()))
            .textures_delta
            .clear();
        if output.settled {
            history.record(program);
        }
        output
    }

    fn code(program: &Program, id: BlockId) -> Option<String> {
        program.find(id).unwrap().inputs["code"].literal.clone()
    }

    #[test]
    fn typing_into_a_field_is_one_step_that_settles_when_the_field_lets_go() {
        let (language, mut program, id, ctx, mut editor, field) = a_code_on_canvas();
        let mut history = block_parse::History::new(&program);
        let mut steps = click(field.center());
        steps.extend(["x", "y"].map(|text| vec![egui::Event::Text(text.into())]));
        let mut settled = false;
        for events in steps {
            settled |= recorded(&ctx, &mut editor, &language, &mut program, &mut history, events).settled;
        }
        assert_eq!(code(&program, id).as_deref(), Some("xy"));
        assert!(!settled, "settled while still typing");

        let mut settled = false;
        for events in click(pos2(700.0, 500.0)) {
            settled |= recorded(&ctx, &mut editor, &language, &mut program, &mut history, events).settled;
        }
        assert!(settled);
        assert_eq!(code(&program, id).as_deref(), Some("XY"), "normalized as it let go");

        assert!(history.undo(&mut program));
        assert_eq!(code(&program, id).as_deref(), Some("ab"), "typing and normalizing are one step");
        assert!(!history.can_undo(&program));
        assert!(history.redo(&mut program));
        assert_eq!(code(&program, id).as_deref(), Some("XY"));
    }

    #[test]
    fn undoing_mid_entry_is_not_normalized_over_as_the_field_lets_go() {
        let (language, mut program, id, ctx, mut editor, field) = a_code_on_canvas();
        let mut history = block_parse::History::new(&program);
        let mut steps = click(field.center());
        steps.push(vec![egui::Event::Text("x".into())]);
        for events in steps {
            recorded(&ctx, &mut editor, &language, &mut program, &mut history, events);
        }
        editor.commit_edit(&ctx, &language, &mut program);
        assert!(history.undo(&mut program));
        assert_eq!(code(&program, id).as_deref(), Some("ab"));
        for _ in 0..3 {
            recorded(&ctx, &mut editor, &language, &mut program, &mut history, vec![]);
        }
        assert_eq!(code(&program, id).as_deref(), Some("ab"), "normalized again after the undo");
        assert!(history.can_redo(&program));
    }

    /// Four closable tabs, `a` to `d`, titled long enough to squeeze each to
    /// a quarter of the default side panel: 80 points from x 880.
    fn four_tabs() -> Overlay {
        let tab = |id: &str| Tab {
            id: TabId::from(id),
            title: format!("{id} with a title far too long to fit in a quarter"),
            closable: true,
            content: TabContent::Text(id.into()),
        };
        Overlay {
            tabs: ["a", "b", "c", "d"].map(tab).into(),
            ..Overlay::default()
        }
    }

    fn tab_center(index: usize) -> Pos2 {
        pos2(880.0 + 80.0 * index as f32 + 30.0, 13.0)
    }

    fn sent(
        ctx: &egui::Context,
        editor: &mut BlockEditor,
        language: &Language,
        program: &mut Program,
        overlay: &Overlay,
        steps: Vec<Vec<egui::Event>>,
    ) -> Vec<EditorEvent> {
        let mut events = Vec::new();
        for input in steps {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
                events: input,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                events.extend(editor.show_with(ui, language, program, overlay).events);
            });
            out.textures_delta.clear();
        }
        events
    }

    fn ids(ids: &[&str]) -> Vec<TabId> {
        ids.iter().map(|&id| TabId::from(id)).collect()
    }

    #[test]
    fn a_tab_is_picked_by_clicking_and_closing_it_is_a_request() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut editor = codon_editor();
        editor.options.side_collapsed = false;
        let overlay = four_tabs();
        sent(&ctx, &mut editor, &language, &mut program, &overlay, vec![vec![]]);
        assert_eq!(editor.options.tab_order, ids(&["a", "b", "c", "d"]));
        assert_eq!(editor.options.active_tab, Some(TabId::from("a")));

        let events = sent(&ctx, &mut editor, &language, &mut program, &overlay, click(tab_center(2)));
        assert_eq!(editor.options.active_tab, Some(TabId::from("c")));
        assert!(events.is_empty(), "{events:?}");

        let close = pos2(880.0 + 80.0 * 2.0 - 13.0, 13.0);
        let events = sent(&ctx, &mut editor, &language, &mut program, &overlay, click(close));
        assert_eq!(events, vec![EditorEvent::CloseTab(TabId::from("b"))]);
        assert_eq!(editor.options.tab_order.len(), 4, "the host has not closed it yet");
    }

    #[test]
    fn dragging_a_tab_reorders_it() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut editor = codon_editor();
        editor.options.side_collapsed = false;
        let overlay = four_tabs();
        frame(&ctx, &mut editor, &language, &mut program, &overlay, vec![]);
        // Past b's middle, short of c's.
        press_and_drag(&ctx, &mut editor, &language, &mut program, &overlay, tab_center(0));
        assert_eq!(editor.options.tab_order, ids(&["b", "a", "c", "d"]));
        assert_eq!(editor.options.active_tab, Some(TabId::from("a")));
        assert!(!editor.is_dragging(), "a tab is not a run");
    }

    #[test]
    fn enter_on_a_console_line_sends_it() {
        let (language, mut program, ctx, _, _) = codon_on_canvas();
        let mut editor = codon_editor();
        editor.options.side_collapsed = false;
        let overlay = Overlay {
            tabs: vec![Tab {
                id: TabId::from("console"),
                title: "Console".into(),
                closable: false,
                content: TabContent::Console { output: "hello\n".into() },
            }],
            ..Overlay::default()
        };
        let mut steps = click(pos2(1000.0, 800.0 - 14.0));
        steps.push(vec![egui::Event::Text("(+ 1 2)".into())]);
        let enter = |pressed| egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        steps.push(vec![enter(true), enter(false)]);
        let events = sent(&ctx, &mut editor, &language, &mut program, &overlay, steps);
        assert_eq!(
            events,
            vec![EditorEvent::ConsoleInput {
                tab: TabId::from("console"),
                line: "(+ 1 2)".into()
            }]
        );
        let line = egui::Id::new("block_editor").with(("console_line", TabId::from("console")));
        assert!(ctx.memory(|memory| memory.has_focus(line)), "the line keeps focus for the next");
    }

    #[test]
    fn the_editor_and_what_it_edits_can_live_in_a_bevy_resource() {
        send_and_sync::<BlockEditor>();
        send_and_sync::<Language>();
        send_and_sync::<Program>();
    }
}
