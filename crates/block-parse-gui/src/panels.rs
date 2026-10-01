//! The palette and inspector either side of the canvas: their widths, the
//! edges that drag to resize them and the buttons that collapse them.

use egui::{Painter, Pos2, Rect, TextEdit, UiBuilder, Vec2, pos2, vec2};

use crate::editor::EditorOptions;
use crate::interact::{DIVIDER_GRIP, Edge};
use crate::theme::Theme;

/// Least widths a panel's edge can be dragged to leave either side.
const MIN_PANEL_WIDTH: f32 = 80.0;
pub(crate) const MIN_CANVAS_WIDTH: f32 = 120.0;
/// A panel's collapse button, half its width inside the canvas from the edge.
pub(crate) const TOGGLE_SIZE: Vec2 = vec2(16.0, 24.0);

/// Screen rects for one frame.
pub(crate) struct Panels {
    pub bounds: Rect,
    pub palette: Rect,
    pub canvas: Rect,
    pub inspector: Rect,
}

impl Panels {
    /// `fitted` is the palette's width when the host gives none.
    pub fn new(bounds: Rect, options: &EditorOptions, fitted: f32) -> Self {
        let room = (bounds.width() - MIN_CANVAS_WIDTH).max(0.0);
        // An open inspector keeps a floor, so it can always be dragged back out.
        let inspector_floor = match options.inspector_collapsed {
            true => 0.0,
            false => options.inspector_width.min(MIN_PANEL_WIDTH),
        };
        let palette_width = match options.palette_width {
            _ if options.palette_collapsed => 0.0,
            Some(width) => width.min((room - inspector_floor).max(0.0)),
            None => fitted.clamp(180.0, 360.0).min(bounds.width() * 0.5),
        };
        let inspector_width = match options.inspector_collapsed {
            true => 0.0,
            false => options.inspector_width.min((room - palette_width).max(0.0)),
        };
        let palette = Rect::from_min_size(bounds.min, vec2(palette_width, bounds.height()));
        let inspector = Rect::from_min_max(pos2(bounds.max.x - inspector_width, bounds.min.y), bounds.max);
        let canvas = Rect::from_min_max(
            pos2(palette.max.x, bounds.min.y),
            pos2(inspector.min.x, bounds.max.y),
        );
        Self {
            bounds,
            palette,
            canvas,
            inspector,
        }
    }

    fn x(&self, edge: Edge) -> f32 {
        match edge {
            Edge::Palette => self.palette.max.x,
            Edge::Inspector => self.inspector.min.x,
        }
    }

    /// The open panel's edge within grabbing distance of `at`.
    pub fn edge_at(&self, options: &EditorOptions, at: Pos2) -> Option<Edge> {
        let near = |edge| (at.x - self.x(edge)).abs() <= DIVIDER_GRIP;
        if !options.palette_collapsed && near(Edge::Palette) {
            Some(Edge::Palette)
        } else if !options.inspector_collapsed && near(Edge::Inspector) {
            Some(Edge::Inspector)
        } else {
            None
        }
    }

    /// The pointer's x minus the edge's, so the edge does not jump.
    pub fn grab(&self, edge: Edge, at: Pos2) -> f32 {
        at.x - self.x(edge)
    }

    /// Moves `edge` to `x`, within what leaves the canvas and the other
    /// panel their room. Takes effect next frame.
    pub fn resize(&self, options: &mut EditorOptions, edge: Edge, x: f32) {
        let room = (self.bounds.width() - MIN_CANVAS_WIDTH).max(0.0);
        let clamp = |width: f32, max: f32| width.clamp(MIN_PANEL_WIDTH.min(max), max);
        match edge {
            Edge::Palette => {
                let max = (room - self.inspector.width()).max(0.0);
                options.palette_width = Some(clamp(x - self.bounds.min.x, max));
            }
            Edge::Inspector => {
                let max = (room - self.palette.width()).max(0.0);
                options.inspector_width = clamp(self.bounds.max.x - x, max);
            }
        }
    }

    fn toggle(&self, edge: Edge) -> Rect {
        let top = self.bounds.min.y + TOGGLE_SIZE.x / 2.0;
        let left = match edge {
            Edge::Palette => self.palette.max.x + TOGGLE_SIZE.x / 2.0,
            Edge::Inspector => self.inspector.min.x - TOGGLE_SIZE.x * 1.5,
        };
        Rect::from_min_size(pos2(left, top), TOGGLE_SIZE)
    }

    pub fn on_toggle(&self, at: Pos2) -> bool {
        [Edge::Palette, Edge::Inspector].into_iter().any(|edge| self.toggle(edge).contains(at))
    }

    /// The collapse buttons. True if one was clicked.
    pub fn toggles(&self, ui: &mut egui::Ui, options: &mut EditorOptions) -> bool {
        let mut ui = ui.new_child(UiBuilder::new().max_rect(self.bounds));
        let mut clicked = false;
        for edge in [Edge::Palette, Edge::Inspector] {
            let (collapsed, glyphs, name) = match edge {
                Edge::Palette => (&mut options.palette_collapsed, ["⏴", "⏵"], "palette"),
                Edge::Inspector => (&mut options.inspector_collapsed, ["⏵", "⏴"], "inspector"),
            };
            let button = egui::Button::new(glyphs[usize::from(*collapsed)]).min_size(TOGGLE_SIZE);
            let tip = format!("{} the {name}", if *collapsed { "Show" } else { "Hide" });
            if ui.put(self.toggle(edge), button).on_hover_text(tip).clicked() {
                *collapsed = !*collapsed;
                clicked = true;
            }
        }
        clicked
    }

    /// Thicker on the edge being dragged or ready to be.
    pub fn edges(&self, painter: &Painter, hot: Option<Edge>, theme: &Theme) {
        for edge in [Edge::Palette, Edge::Inspector] {
            let width = if hot == Some(edge) { 3.0 } else { 1.0 };
            painter.vline(self.x(edge), self.bounds.y_range(), (width, theme.divider));
        }
    }

    /// Wraps rather than scrolling sideways, and fills the panel even when
    /// short. Read-only, but it can be selected and copied.
    pub fn inspector(&self, ui: &mut egui::Ui, id: egui::Id, text: &str, theme: &Theme) {
        if self.inspector.width() <= 0.0 {
            return;
        }
        let mut panel = ui.new_child(UiBuilder::new().max_rect(self.inspector));
        panel.set_clip_rect(self.inspector);
        panel.painter().rect_filled(self.inspector, 0.0, theme.palette);
        egui::ScrollArea::vertical()
            .id_salt(id.with("inspector"))
            .auto_shrink(false)
            .show(&mut panel, |ui| {
                let mut shown = text;
                let text = TextEdit::multiline(&mut shown)
                    .id(id.with("inspector_text"))
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .min_size(ui.available_size());
                ui.add(text);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_palette_leaves_an_open_inspector_room_to_drag() {
        let bounds = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        let options = EditorOptions {
            palette_width: Some(5000.0),
            inspector_collapsed: false,
            ..EditorOptions::default()
        };
        let panels = Panels::new(bounds, &options, 200.0);
        assert_eq!(panels.inspector.width(), MIN_PANEL_WIDTH);
        assert_eq!(panels.canvas.width(), MIN_CANVAS_WIDTH);
    }
}
