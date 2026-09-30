//! Scene to egui shapes. Everything is laid out in canvas units and mapped
//! through a [`Transform`] here, fonts included.

use std::collections::HashMap;

use block_parse::host::{Highlight, Overlay};
use block_parse::language::{LiteralKind, Shape};
use block_parse::program::BlockId;
use egui::epaint::Mesh;
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

use crate::interact::SnapMark;
use crate::layout::{Form, LABEL_SIZE, LITERAL_SIZE, PlacedBlock, PlacedSlot, Scene, Section, SlotContent};
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

const ACCENT_WIDTH: f32 = 3.0;
/// Under every accent, so it never depends on contrast with the block.
const HALO_WIDTH: f32 = ACCENT_WIDTH + 2.5;

/// `live` marks literals that real widgets will cover: only their backgrounds
/// are painted, or the text would show twice. Choices are always painted.
pub fn scene(painter: &Painter, scene: &Scene, t: Transform, theme: &Theme, live: bool, overlay: &Overlay) {
    let highlights = by_block(overlay);
    for block in &scene.blocks {
        let accent = highlights.get(&block.id).map(|h| theme.highlight(h.style));
        let muted = overlay.muted.contains(&block.id);
        paint_block(painter, block, t, theme, live, accent, muted);
        // A switch with host state gets a live checkbox; one without is
        // drawn disabled.
        if let Some(rect) = block.switch
            && !overlay.switches.contains_key(&block.id)
        {
            let check = Rect::from_center_size(t.pos(rect.center()), vec2(14.0, 14.0) * t.zoom);
            painter.rect(
                check,
                radius(2.0 * t.zoom),
                block.swatch.shadow,
                Stroke::new(t.zoom, block.swatch.edge),
                StrokeKind::Inside,
            );
        }
    }
}

/// Breakpoints, highlight labels and annotations, drawn over every block.
pub fn markers(painter: &Painter, scene: &Scene, t: Transform, theme: &Theme, overlay: &Overlay) {
    let highlights = by_block(overlay);
    for block in &scene.blocks {
        let row = pos2(block.rect.max.x, first_row_center(block));
        let mut tags = 0.0;
        if overlay.breakpoints.contains(&block.id) {
            let at = t.pos(pos2(block.rect.min.x, row.y)) - vec2(8.0 * t.zoom, 0.0);
            painter.circle(at, 5.0 * t.zoom, theme.breakpoint, Stroke::new(1.5 * t.zoom, theme.halo));
        }
        if let Some(highlight) = highlights.get(&block.id)
            && let Some(label) = &highlight.label
        {
            let fill = theme.highlight(highlight.style);
            tag(painter, t.pos(row) + vec2(6.0 * t.zoom, 0.0), label, fill, theme.literal_ink, t.zoom);
            tags += 1.0;
        }
        for annotation in overlay.annotations.iter().filter(|a| a.block == block.id) {
            let fill = match annotation.severity {
                block_parse::ast::Severity::Error => theme.error,
                block_parse::ast::Severity::Warning => theme.warning,
            };
            let at = t.pos(row) + vec2(6.0, 18.0 * tags) * t.zoom;
            tag(painter, at, &annotation.message, fill, Color32::WHITE, t.zoom);
            tags += 1.0;
        }
    }
}

/// Where one block has several highlights, the last wins.
fn by_block(overlay: &Overlay) -> HashMap<BlockId, &Highlight> {
    overlay.highlights.iter().map(|highlight| (highlight.block, highlight)).collect()
}

fn first_row_center(block: &PlacedBlock) -> f32 {
    match &block.form {
        Form::Stack(form) => match form.sections.first() {
            Some(Section::Row { top, bottom } | Section::Branch { top, bottom }) => (top + bottom) / 2.0,
            None => block.rect.center().y,
        },
        Form::Reporter(_) => block.rect.center().y,
    }
}

/// A small label, its left edge vertically centered on `left_center`.
fn tag(painter: &Painter, left_center: Pos2, text: &str, fill: Color32, ink: Color32, zoom: f32) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(11.0 * zoom), ink);
    let size = galley.size() + vec2(8.0, 4.0) * zoom;
    let rect = Rect::from_min_size(left_center - vec2(0.0, size.y / 2.0), size);
    painter.rect_filled(rect, radius(3.0 * zoom), fill);
    painter.galley(rect.min + vec2(4.0, 2.0) * zoom, galley, ink);
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

/// An `accent` replaces the block's own edge, so a highlighted block keeps its
/// shape and size.
fn paint_block(
    painter: &Painter,
    block: &PlacedBlock,
    t: Transform,
    theme: &Theme,
    live: bool,
    accent: Option<Color32>,
    muted: bool,
) {
    let swatch = block.swatch;
    let (body, edge_color) = if muted {
        (swatch.muted, swatch.muted_edge)
    } else {
        (swatch.fill, swatch.edge)
    };
    let edge = Stroke::new((1.0 * t.zoom).max(1.0), edge_color);
    let strokes = |accent: Color32| {
        [
            Stroke::new(HALO_WIDTH * t.zoom, theme.halo),
            Stroke::new(ACCENT_WIDTH * t.zoom, accent),
        ]
    };
    match &block.form {
        Form::Stack(form) => {
            let pieces = shape::stack_fill(block.rect, form).into_iter().map(|piece| t.points(piece));
            fill_pieces(painter, pieces, body);
            let outline = t.points(shape::stack_outline(block.rect, form));
            match accent {
                Some(accent) => {
                    for stroke in strokes(accent) {
                        painter.add(egui::Shape::closed_line(outline.clone(), stroke));
                    }
                }
                None => {
                    painter.add(egui::Shape::closed_line(outline, edge));
                }
            }
        }
        Form::Reporter(shape) => {
            let rect = t.rect(block.rect);
            fill(painter, *shape, rect, body, edge, t.zoom);
            if let Some(accent) = accent {
                for stroke in strokes(accent) {
                    outline(painter, *shape, rect, stroke, t.zoom);
                }
            }
        }
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
    if live && !matches!(kind, LiteralKind::Choice(_)) {
        return;
    }

    let font = FontId::proportional(LITERAL_SIZE * t.zoom);
    match kind {
        LiteralKind::Bool => {
            let check = Rect::from_center_size(rect.center(), vec2(14.0, 14.0) * t.zoom);
            painter.rect_filled(check, radius(2.0 * t.zoom), theme.literal_fill);
            if text == "true" {
                tick(painter, check.center(), 14.0 * t.zoom, Stroke::new(2.0 * t.zoom, theme.literal_ink));
            }
        }
        LiteralKind::Choice(_) => {
            painter.text(
                rect.left_center() + vec2(8.0 * t.zoom, 0.0),
                Align2::LEFT_CENTER,
                text,
                font,
                theme.literal_ink,
            );
            let caret = pos2(rect.max.x - 12.0 * t.zoom, rect.center().y);
            let caret = [vec2(-4.0, -2.0), vec2(4.0, -2.0), vec2(0.0, 3.0)].map(|v| caret + v * t.zoom);
            painter.add(egui::Shape::convex_polygon(caret.to_vec(), theme.literal_ink, Stroke::NONE));
        }
        _ => {
            painter.text(rect.center(), Align2::CENTER_CENTER, text, font, theme.literal_ink);
        }
    }
}

/// Screen-space convex pieces as one mesh, so shared edges leave no seams.
pub fn fill_pieces(painter: &Painter, pieces: impl IntoIterator<Item = Vec<Pos2>>, color: Color32) {
    let mut mesh = Mesh::default();
    for piece in pieces {
        let base = mesh.vertices.len() as u32;
        for point in &piece {
            mesh.colored_vertex(*point, color);
        }
        for i in 1..piece.len() as u32 - 1 {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

pub fn tick(painter: &Painter, center: Pos2, size: f32, stroke: Stroke) {
    let check = Rect::from_center_size(center, vec2(size, size));
    let inset = size * 3.0 / 14.0;
    let tick = [
        pos2(check.min.x + inset, check.center().y),
        pos2(check.center().x - size / 14.0, check.max.y - inset),
        pos2(check.max.x - inset, check.min.y + inset),
    ];
    painter.add(egui::Shape::line(tick.to_vec(), stroke));
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
