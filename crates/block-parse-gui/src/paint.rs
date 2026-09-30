//! Scene to egui shapes. Everything is laid out in canvas units and mapped
//! through a [`Transform`] here, fonts included.

use block_parse::language::{LiteralKind, Shape};
use egui::epaint::Mesh;
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

use crate::interact::SnapMark;
use crate::layout::{Form, LABEL_SIZE, LITERAL_SIZE, PlacedBlock, PlacedSlot, Scene, SlotContent};
use crate::shape::{self, TopEdge};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Screen position of the canvas origin.
    pub origin: Pos2,
    pub zoom: f32,
}

impl Transform {
    pub fn pos(&self, point: Pos2) -> Pos2 {
        self.origin + point.to_vec2() * self.zoom
    }

    pub fn rect(&self, rect: Rect) -> Rect {
        Rect::from_min_max(self.pos(rect.min), self.pos(rect.max))
    }

    pub fn canvas(&self, screen: Pos2) -> Pos2 {
        ((screen - self.origin) / self.zoom).to_pos2()
    }

    fn points(&self, points: Vec<Pos2>) -> Vec<Pos2> {
        points.into_iter().map(|point| self.pos(point)).collect()
    }
}

/// `live` marks literals that real widgets will cover: only their backgrounds
/// are painted, or the text would show twice.
pub fn scene(painter: &Painter, scene: &Scene, t: Transform, theme: &Theme, live: bool) {
    for block in &scene.blocks {
        paint_block(painter, block, t, theme, live);
    }
}

pub fn error_tags(painter: &Painter, scene: &Scene, t: Transform, theme: &Theme) {
    for slot in scene.slots() {
        let SlotContent::Literal {
            error: Some(message),
            ..
        } = &slot.content
        else {
            continue;
        };
        let font = FontId::proportional(11.0 * t.zoom);
        let galley = painter.layout_no_wrap(message.clone(), font, Color32::WHITE);
        let rect = t.rect(slot.rect);
        let at = pos2(rect.min.x, rect.max.y + 3.0 * t.zoom);
        let tag = Rect::from_min_size(at, galley.size() + vec2(8.0, 4.0) * t.zoom);
        painter.rect_filled(tag, radius(3.0 * t.zoom), theme.error);
        painter.galley(at + vec2(4.0, 2.0) * t.zoom, galley, Color32::WHITE);
    }
}

pub fn snap_mark(painter: &Painter, mark: &SnapMark, t: Transform, theme: &Theme) {
    let stroke = Stroke::new(3.0 * t.zoom, theme.snap);
    match mark {
        SnapMark::Seam { at, width } => {
            let edge = shape::top_edge(at.x, *width, at.y, TopEdge::Notched);
            painter.add(egui::Shape::line(t.points(edge), stroke));
        }
        SnapMark::Slot { rect, shape } => outline(painter, *shape, t.rect(*rect), stroke, t.zoom),
    }
}

fn paint_block(painter: &Painter, block: &PlacedBlock, t: Transform, theme: &Theme, live: bool) {
    let swatch = block.swatch;
    let edge = Stroke::new((1.0 * t.zoom).max(1.0), swatch.edge);
    match &block.form {
        Form::Stack(form) => {
            let mut mesh = Mesh::default();
            for piece in shape::stack_fill(block.rect, form) {
                let base = mesh.vertices.len() as u32;
                for point in &piece {
                    mesh.colored_vertex(t.pos(*point), swatch.fill);
                }
                for i in 1..piece.len() as u32 - 1 {
                    mesh.add_triangle(base, base + i, base + i + 1);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
            let outline = t.points(shape::stack_outline(block.rect, form));
            painter.add(egui::Shape::closed_line(outline, edge));
        }
        Form::Reporter(shape) => fill(painter, *shape, t.rect(block.rect), swatch.fill, edge, t.zoom),
    }

    let font = FontId::proportional(LABEL_SIZE * t.zoom);
    for label in &block.labels {
        painter.text(t.pos(label.at), Align2::LEFT_CENTER, &label.text, font.clone(), swatch.ink);
    }
    for slot in &block.slots {
        paint_slot(painter, slot, t, theme, live);
    }
}

fn paint_slot(painter: &Painter, slot: &PlacedSlot, t: Transform, theme: &Theme, live: bool) {
    let rect = t.rect(slot.rect);
    let edge = Stroke::new((1.0 * t.zoom).max(1.0), slot.swatch.edge);
    let (kind, text, error) = match &slot.content {
        SlotContent::Plugged(_) => return,
        SlotContent::Empty => {
            fill(painter, slot.shape, rect, slot.swatch.shadow, edge, t.zoom);
            return;
        }
        SlotContent::Literal { kind, text, error } => (kind, text, error),
    };

    let background = if *kind == LiteralKind::Bool {
        slot.swatch.shadow
    } else {
        theme.literal_fill
    };
    fill(painter, slot.shape, rect, background, edge, t.zoom);
    if error.is_some() {
        outline(painter, slot.shape, rect, Stroke::new(2.0 * t.zoom, theme.error), t.zoom);
    }
    if live {
        return;
    }

    let font = FontId::proportional(LITERAL_SIZE * t.zoom);
    match kind {
        LiteralKind::Bool => {
            let check = Rect::from_center_size(rect.center(), vec2(14.0, 14.0) * t.zoom);
            painter.rect_filled(check, radius(2.0 * t.zoom), theme.literal_fill);
            if text == "true" {
                let tick = [
                    pos2(check.min.x + 3.0 * t.zoom, check.center().y),
                    pos2(check.center().x - t.zoom, check.max.y - 3.0 * t.zoom),
                    pos2(check.max.x - 3.0 * t.zoom, check.min.y + 3.0 * t.zoom),
                ];
                painter.add(egui::Shape::line(tick.to_vec(), Stroke::new(2.0 * t.zoom, theme.literal_ink)));
            }
        }
        LiteralKind::Choice(_) => {
            painter.text(
                rect.left_center() + vec2(8.0 * t.zoom, 0.0),
                Align2::LEFT_CENTER,
                format!("{text} \u{25be}"),
                font,
                theme.literal_ink,
            );
        }
        _ => {
            painter.text(rect.center(), Align2::CENTER_CENTER, text, font, theme.literal_ink);
        }
    }
}

fn fill(painter: &Painter, shape: Shape, rect: Rect, color: Color32, edge: Stroke, zoom: f32) {
    match shape {
        Shape::Round => {
            painter.rect(rect, radius(rect.height() / 2.0), color, edge, StrokeKind::Inside);
        }
        Shape::Square => {
            painter.rect(rect, radius(3.0 * zoom), color, edge, StrokeKind::Inside);
        }
        Shape::Hexagon => {
            painter.add(egui::Shape::convex_polygon(shape::hexagon(rect), color, edge));
        }
    }
}

fn outline(painter: &Painter, shape: Shape, rect: Rect, stroke: Stroke, zoom: f32) {
    match shape {
        Shape::Round => {
            painter.rect_stroke(rect, radius(rect.height() / 2.0), stroke, StrokeKind::Outside);
        }
        Shape::Square => {
            painter.rect_stroke(rect, radius(3.0 * zoom), stroke, StrokeKind::Outside);
        }
        Shape::Hexagon => {
            painter.add(egui::Shape::closed_line(shape::hexagon(rect), stroke));
        }
    }
}

fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}
