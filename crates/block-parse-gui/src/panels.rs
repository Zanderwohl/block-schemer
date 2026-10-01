//! The palette and side panel either side of the canvas: their widths, the
//! edges that drag to resize them and the buttons that collapse them.

use egui::{Painter, Pos2, Rect, UiBuilder, Vec2, pos2, vec2};

use crate::editor::EditorOptions;
use crate::interact::{DIVIDER_GRIP, Edge};
use crate::theme::Theme;

/// Least widths a panel's edge can be dragged to leave either side.
const MIN_PANEL_WIDTH: f32 = 80.0;
pub(crate) const MIN_CANVAS_WIDTH: f32 = 120.0;
pub(crate) const TOGGLE_SIZE: Vec2 = vec2(16.0, 24.0);

/// Screen rects for one frame.
pub(crate) struct Panels {
    pub bounds: Rect,
    pub palette: Rect,
    pub canvas: Rect,
    pub side: Rect,
}

impl Panels {
    /// `fitted` is the palette's width when the host gives none.
    pub fn new(bounds: Rect, options: &EditorOptions, fitted: f32) -> Self {
        let room = (bounds.width() - MIN_CANVAS_WIDTH).max(0.0);
        // An open side panel keeps a floor, so it can always be dragged back out.
        let side_floor = match options.side_collapsed {
            true => 0.0,
            false => options.side_width.min(MIN_PANEL_WIDTH),
        };
        let palette_width = match options.palette_width {
            _ if options.palette_collapsed => 0.0,
            Some(width) => width.min((room - side_floor).max(0.0)),
            None => fitted.clamp(180.0, 360.0).min(bounds.width() * 0.5),
        };
        let side_width = match options.side_collapsed {
            true => 0.0,
            false => options.side_width.min((room - palette_width).max(0.0)),
        };
        let palette = Rect::from_min_size(bounds.min, vec2(palette_width, bounds.height()));
        let side = Rect::from_min_max(pos2(bounds.max.x - side_width, bounds.min.y), bounds.max);
        let canvas = Rect::from_min_max(
            pos2(palette.max.x, bounds.min.y),
            pos2(side.min.x, bounds.max.y),
        );
        Self {
            bounds,
            palette,
            canvas,
            side,
        }
    }

    fn x(&self, edge: Edge) -> f32 {
        match edge {
            Edge::Palette => self.palette.max.x,
            Edge::Side => self.side.min.x,
        }
    }

    pub fn edge_at(&self, options: &EditorOptions, at: Pos2) -> Option<Edge> {
        let near = |edge| (at.x - self.x(edge)).abs() <= DIVIDER_GRIP;
        if !options.palette_collapsed && near(Edge::Palette) {
            Some(Edge::Palette)
        } else if !options.side_collapsed && near(Edge::Side) {
            Some(Edge::Side)
        } else {
            None
        }
    }

    pub fn grab(&self, edge: Edge, at: Pos2) -> f32 {
        at.x - self.x(edge)
    }

    /// Takes effect next frame.
    pub fn resize(&self, options: &mut EditorOptions, edge: Edge, x: f32) {
        let room = (self.bounds.width() - MIN_CANVAS_WIDTH).max(0.0);
        let clamp = |width: f32, max: f32| width.clamp(MIN_PANEL_WIDTH.min(max), max);
        match edge {
            Edge::Palette => {
                let max = (room - self.side.width()).max(0.0);
                options.palette_width = Some(clamp(x - self.bounds.min.x, max));
            }
            Edge::Side => {
                let max = (room - self.palette.width()).max(0.0);
                options.side_width = clamp(self.bounds.max.x - x, max);
            }
        }
    }

    fn toggle(&self, edge: Edge) -> Rect {
        let top = self.bounds.min.y + TOGGLE_SIZE.x / 2.0;
        let left = match edge {
            Edge::Palette => self.palette.max.x + TOGGLE_SIZE.x / 2.0,
            Edge::Side => self.side.min.x - TOGGLE_SIZE.x * 1.5,
        };
        Rect::from_min_size(pos2(left, top), TOGGLE_SIZE)
    }

    pub fn on_toggle(&self, at: Pos2) -> bool {
        [Edge::Palette, Edge::Side].into_iter().any(|edge| self.toggle(edge).contains(at))
    }

    /// True if one was clicked.
    pub fn toggles(&self, ui: &mut egui::Ui, options: &mut EditorOptions) -> bool {
        let mut ui = ui.new_child(UiBuilder::new().max_rect(self.bounds));
        let mut clicked = false;
        for edge in [Edge::Palette, Edge::Side] {
            let (collapsed, glyphs, name) = match edge {
                Edge::Palette => (&mut options.palette_collapsed, ["⏴", "⏵"], "palette"),
                Edge::Side => (&mut options.side_collapsed, ["⏵", "⏴"], "side panel"),
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

    pub fn edges(&self, painter: &Painter, hot: Option<Edge>, theme: &Theme) {
        for edge in [Edge::Palette, Edge::Side] {
            let width = if hot == Some(edge) { 3.0 } else { 1.0 };
            painter.vline(self.x(edge), self.bounds.y_range(), (width, theme.divider));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_palette_leaves_an_open_side_panel_room_to_drag() {
        let bounds = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        let options = EditorOptions {
            palette_width: Some(5000.0),
            side_collapsed: false,
            ..EditorOptions::default()
        };
        let panels = Panels::new(bounds, &options, 200.0);
        assert_eq!(panels.side.width(), MIN_PANEL_WIDTH);
        assert_eq!(panels.canvas.width(), MIN_CANVAS_WIDTH);
    }
}
