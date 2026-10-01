//! Scene to egui shapes. Everything is laid out in canvas units and mapped
//! through a [`Transform`] here, fonts included.

use std::collections::HashMap;
use std::sync::Arc;

use block_parse::host::{Highlight, Overlay};
use block_parse::language::{LiteralKind, Shape};
use block_parse::program::BlockId;
use egui::epaint::Mesh;
use egui::{Align2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

use crate::bubble::{self, Bubble};
use crate::interact::SnapMark;
use crate::layout::{FAINT_SIZE, Form, LABEL_SIZE, LITERAL_SIZE, append_text, is_blank, PlacedBlock, PlacedSlot, Scene, Section, SlotContent};
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
const BEVEL_WIDTH: f32 = 2.0;
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
        Form::Reporter { first_row, .. } => block.rect.min.y + first_row,
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

const BUBBLE_SIZE: f32 = 13.0;
/// Canvas units; longer text wraps.
const BUBBLE_WRAP: f32 = 220.0;
const BUBBLE_SHADOW: f32 = 2.0;

/// In canvas units, each kept off earlier ones. Text is laid out at `zoom`.
pub fn place_bubbles(
    painter: &Painter,
    scene: &Scene,
    zoom: f32,
    theme: &Theme,
    overlay: &Overlay,
    visible: Rect,
) -> Vec<(Bubble, Arc<Galley>)> {
    let mut obstacles: Vec<Rect> = scene.blocks.iter().flat_map(|block| block.hit.iter().copied()).collect();
    let mut placed = Vec::new();
    for block in &scene.blocks {
        let Some(text) = overlay.bubbles.get(&block.id) else {
            continue;
        };
        let font = FontId::proportional(BUBBLE_SIZE * zoom);
        let galley = painter.layout(text.clone(), font, theme.literal_ink, BUBBLE_WRAP * zoom);
        let size = galley.size() / zoom + 2.0 * bubble::PADDING;
        let head = block.hit.first().copied().unwrap_or(block.rect);
        let bubble = bubble::place(head, block.rect, size, &obstacles, visible);
        obstacles.push(bubble.body);
        placed.push((bubble, galley));
    }
    placed
}

/// Bubbles from [`place_bubbles`] at the same zoom.
pub fn bubbles(painter: &Painter, placed: Vec<(Bubble, Arc<Galley>)>, t: Transform, theme: &Theme) {
    for (bubble, galley) in placed {
        let shadow = bubble.translate(vec2(0.0, BUBBLE_SHADOW));
        fill_pieces(painter, shadow.fill().into_iter().map(|piece| t.points(piece)), theme.halo);
        fill_pieces(painter, bubble.fill().into_iter().map(|piece| t.points(piece)), theme.literal_fill);
        let edge = Stroke::new(t.zoom.max(1.0), theme.halo);
        painter.add(egui::Shape::closed_line(t.points(bubble.outline()), edge));
        painter.galley(t.pos(bubble.body.min + bubble::PADDING), galley, theme.literal_ink);
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
        SnapMark::Slot { rect, shape } => {
            let rect = t.rect(*rect);
            outline(painter, *shape, rect, rect.height(), stroke, t.zoom);
        }
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
    let (body, edge_color, light, dark) = if muted {
        (swatch.muted, swatch.muted_edge, swatch.muted_highlight, swatch.muted_shadow)
    } else {
        (swatch.fill, swatch.edge, swatch.highlight, swatch.shadow)
    };
    let bevel = |outline: &[Pos2]| {
        bevel(painter, outline, BEVEL_WIDTH * t.zoom, body, light, dark);
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
            bevel(&outline);
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
        Form::Reporter { shape, head, .. } => {
            let rect = t.rect(block.rect);
            let head = head * t.zoom;
            // Not `painter.rect`: it snaps to pixels and the bevel would not.
            let points = reporter_outline(*shape, rect, head, t.zoom);
            painter.add(egui::Shape::convex_polygon(points.clone(), body, Stroke::NONE));
            bevel(&points);
            painter.add(egui::Shape::closed_line(points, edge));
            if let Some(accent) = accent {
                for stroke in strokes(accent) {
                    outline(painter, *shape, rect, head, stroke, t.zoom);
                }
            }
        }
    }

    for label in &block.labels {
        let (size, ink) = match label.faint {
            true => (FAINT_SIZE, swatch.ink.gamma_multiply(0.6)),
            false => (LABEL_SIZE, swatch.ink),
        };
        let font = FontId::proportional(size * t.zoom);
        painter.text(t.pos(label.at), Align2::LEFT_CENTER, &label.text, font, ink);
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
            fill(painter, slot.shape, rect, rect.height(), slot.swatch.shadow, edge, t.zoom);
            hint(painter, rect, &slot.hint, slot.swatch.ink.gamma_multiply(0.6), t.zoom);
            return;
        }
        SlotContent::Append { .. } => {
            outline(painter, slot.shape, rect.shrink(edge.width), rect.height(), edge, t.zoom);
            hint(painter, rect, &append_text(&slot.hint), slot.swatch.ink.gamma_multiply(0.75), t.zoom);
            return;
        }
        SlotContent::Literal { kind, text, error } => (kind, text, error),
    };

    let background = if *kind == LiteralKind::Bool {
        slot.swatch.shadow
    } else {
        theme.literal_fill
    };
    fill(painter, slot.shape, rect, rect.height(), background, edge, t.zoom);
    if error.is_some() {
        outline(painter, slot.shape, rect, rect.height(), Stroke::new(2.0 * t.zoom, theme.error), t.zoom);
    }
    // Under a live field too: the field is transparent and draws no hint.
    if is_blank(text) && !matches!(kind, LiteralKind::Bool | LiteralKind::Choice(_)) {
        hint(painter, rect, &slot.hint, theme.placeholder, t.zoom);
        return;
    }
    if live && slot.is_field() && !matches!(kind, LiteralKind::Choice(_)) {
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

fn hint(painter: &Painter, rect: Rect, text: &str, color: Color32, zoom: f32) {
    let font = FontId::proportional(LITERAL_SIZE * zoom);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

/// Lit from the top left: sides facing up or left get all of `light`, down or
/// right all of `dark`, the other diagonal stays `base`.
fn bevel(painter: &Painter, outline: &[Pos2], width: f32, base: Color32, light: Color32, dark: Color32) {
    let toward_light = vec2(-1.0, -1.0);
    let mut mesh = Mesh::default();
    for (piece, normal) in shape::bevel(outline, width) {
        let facing = normal.dot(toward_light).clamp(-1.0, 1.0);
        let color = if facing >= 0.0 {
            base.lerp_to_gamma(light, facing)
        } else {
            base.lerp_to_gamma(dark, -facing)
        };
        let first = mesh.vertices.len() as u32;
        for point in piece {
            mesh.colored_vertex(point, color);
        }
        mesh.add_triangle(first, first + 1, first + 2);
        mesh.add_triangle(first, first + 2, first + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
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

/// `head` sizes the ends.
fn fill(painter: &Painter, shape: Shape, rect: Rect, head: f32, color: Color32, edge: Stroke, zoom: f32) {
    match shape {
        Shape::Round => {
            painter.rect(rect, radius(head / 2.0), color, edge, StrokeKind::Inside);
        }
        Shape::Square => {
            painter.rect(rect, radius(3.0 * zoom), color, edge, StrokeKind::Inside);
        }
        Shape::Hexagon => {
            painter.add(egui::Shape::convex_polygon(shape::hexagon(rect, head), color, edge));
        }
    }
}

/// Clockwise, with the corners [`fill`] gives `shape`.
fn reporter_outline(shape: Shape, rect: Rect, head: f32, zoom: f32) -> Vec<Pos2> {
    let corner = match shape {
        Shape::Round => head / 2.0,
        Shape::Square => 3.0 * zoom,
        Shape::Hexagon => return shape::hexagon(rect, head),
    };
    shape::rounded_rect(rect, corner)
}

fn outline(painter: &Painter, shape: Shape, rect: Rect, head: f32, stroke: Stroke, zoom: f32) {
    match shape {
        Shape::Round => {
            painter.rect_stroke(rect, radius(head / 2.0), stroke, StrokeKind::Outside);
        }
        Shape::Square => {
            painter.rect_stroke(rect, radius(3.0 * zoom), stroke, StrokeKind::Outside);
        }
        Shape::Hexagon => {
            painter.add(egui::Shape::closed_line(shape::hexagon(rect, head), stroke));
        }
    }
}

fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}
