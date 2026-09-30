//! The menu of a choice literal, drawn with the geometry and swatch of its
//! block. Sizes here are at scale 1.

use egui::{Align2, Area, Color32, CornerRadius, CursorIcon, FontId, Order, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::color::Swatch;
use crate::layout::LITERAL_SIZE;
use crate::paint;
use crate::shape;

pub const ROW_HEIGHT: f32 = 24.0;
pub const INSET: f32 = 4.0;
pub const TEXT_INSET: f32 = 10.0;
/// Reserved on every row, so text stays put when the selection moves.
pub const CHECK_WIDTH: f32 = 28.0;
/// The pointer's height; its base is twice this.
pub const POINTER: f32 = 7.0;
/// Between the anchor and the pointer's tip.
pub const GAP: f32 = 2.0;
const SHADOW_DROP: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// Screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub side: Side,
    pub body: Rect,
    pub tip: Pos2,
    /// List the body cannot show: the scroll range.
    pub overflow: f32,
}

impl Placement {
    pub fn outer(&self) -> Rect {
        self.body.union(Rect::from_min_max(self.tip, self.tip))
    }
}

/// Below `anchor` if the menu fits there, else above if it fits there, else
/// on the roomier side, cut short to scroll.
pub fn place(anchor: Rect, size: Vec2, bounds: Rect, scale: f32) -> Placement {
    let reach = (POINTER + GAP) * scale;
    let below = bounds.max.y - anchor.max.y - reach;
    let above = anchor.min.y - bounds.min.y - reach;
    let side = if size.y <= below || (size.y > above && below >= above) {
        Side::Below
    } else {
        Side::Above
    };
    let room = match side {
        Side::Below => below,
        Side::Above => above,
    };
    let height = size.y.min(room.max((ROW_HEIGHT + 2.0 * INSET) * scale));
    let width = size.x.min(bounds.width());
    let x = anchor.min.x.min(bounds.max.x - width).max(bounds.min.x);
    let (top, tip_y) = match side {
        Side::Below => (anchor.max.y + reach, anchor.max.y + GAP * scale),
        Side::Above => (anchor.min.y - reach - height, anchor.min.y - GAP * scale),
    };
    let body = Rect::from_min_size(pos2(x, top), vec2(width, height));
    // Keeps the pointer's base clear of the corners.
    let margin = 2.0 * POINTER * scale;
    let tip_x = anchor.center().x.min(body.max.x - margin).max(body.min.x + margin);
    Placement {
        side,
        body,
        tip: pos2(tip_x, tip_y),
        overflow: size.y - height,
    }
}

/// The body's full size, before any cut to fit the screen.
pub fn size(widest_text: f32, options: usize, anchor_width: f32, scale: f32) -> Vec2 {
    vec2(
        ((TEXT_INSET + widest_text + CHECK_WIDTH) * scale).max(anchor_width),
        (2.0 * INSET + options as f32 * ROW_HEIGHT) * scale,
    )
}

pub fn row(body: Rect, index: usize, scroll: f32, scale: f32) -> Rect {
    let top = body.min.y + (INSET + index as f32 * ROW_HEIGHT) * scale - scroll;
    Rect::from_min_size(pos2(body.min.x, top), vec2(body.width(), ROW_HEIGHT * scale))
}

pub fn row_at(body: Rect, point: Pos2, options: usize, scroll: f32, scale: f32) -> Option<usize> {
    if !body.contains(point) {
        return None;
    }
    let offset = point.y - body.min.y - INSET * scale + scroll;
    let index = (offset / (ROW_HEIGHT * scale)).floor();
    (index >= 0.0 && (index as usize) < options).then_some(index as usize)
}

pub struct Menu<'a> {
    /// Also the menu's layer.
    pub id: egui::Id,
    pub options: &'a [String],
    pub selected: &'a str,
    /// The block's, so the menu reads as part of it.
    pub swatch: Swatch,
    pub shadow: Color32,
    pub scale: f32,
}

impl Menu<'_> {
    /// Shows the menu against `anchor` for this frame. Returns the index of
    /// the option clicked, if any.
    pub fn show(&self, ctx: &egui::Context, anchor: Rect, scroll: &mut f32) -> Option<usize> {
        let s = self.scale;
        let font = FontId::proportional(LITERAL_SIZE * s);
        let widest = ctx.fonts_mut(|fonts| {
            self.options
                .iter()
                .map(|option| fonts.layout_no_wrap(option.clone(), font.clone(), Color32::WHITE).size().x)
                .fold(0.0, f32::max)
        });
        let size = size(widest / s, self.options.len(), anchor.width(), s);
        let placed = place(anchor, size, ctx.content_rect(), s);

        let mut chosen = None;
        Area::new(self.id)
            .order(Order::Foreground)
            .fixed_pos(placed.outer().min)
            .constrain(false)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(placed.outer(), Sense::click());
                if response.hovered() {
                    *scroll -= ui.input(|input| input.smooth_scroll_delta.y);
                }
                *scroll = scroll.clamp(0.0, placed.overflow);
                let hovered = response
                    .hover_pos()
                    .and_then(|at| row_at(placed.body, at, self.options.len(), *scroll, s));
                if hovered.is_some() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if response.clicked() {
                    chosen = hovered;
                }
                self.paint(ui.painter(), &placed, *scroll, hovered, &font);
            });
        chosen
    }

    fn paint(&self, painter: &egui::Painter, placed: &Placement, scroll: f32, hovered: Option<usize>, font: &FontId) {
        let s = self.scale;
        let half_base = POINTER * s;
        let drop = vec2(0.0, SHADOW_DROP * s);
        let shadow = shape::callout_fill(placed.body.translate(drop), placed.tip + drop, half_base);
        paint::fill_pieces(painter, shadow, self.shadow);
        paint::fill_pieces(painter, shape::callout_fill(placed.body, placed.tip, half_base), self.swatch.fill);
        let edge = Stroke::new(s.max(1.0), self.swatch.edge);
        painter.add(egui::Shape::closed_line(
            shape::callout_outline(placed.body, placed.tip, half_base),
            edge,
        ));

        let list = painter.with_clip_rect(placed.body.shrink(s.max(1.0)));
        for (index, option) in self.options.iter().enumerate() {
            let rect = row(placed.body, index, scroll, s);
            if !list.clip_rect().intersects(rect) {
                continue;
            }
            if hovered == Some(index) {
                let band = rect.shrink2(vec2(INSET * s, 1.0 * s));
                list.rect_filled(band, CornerRadius::same((3.0 * s) as u8), self.swatch.shadow);
            }
            let text_at = pos2(rect.min.x + TEXT_INSET * s, rect.center().y);
            list.text(text_at, Align2::LEFT_CENTER, option, font.clone(), self.swatch.ink);
            if option == self.selected {
                let center = pos2(rect.max.x - CHECK_WIDTH * s / 2.0, rect.center().y);
                paint::tick(&list, center, 14.0 * s, Stroke::new(2.0 * s, self.swatch.ink));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));

    fn anchor_at(y: f32) -> Rect {
        Rect::from_min_size(pos2(100.0, y), vec2(60.0, 22.0))
    }

    #[test]
    fn a_menu_opens_below_when_it_fits() {
        let placed = place(anchor_at(100.0), vec2(120.0, 200.0), SCREEN, 1.0);
        assert_eq!(placed.side, Side::Below);
        assert!(placed.tip.y > anchor_at(100.0).max.y && placed.tip.y < placed.body.min.y);
        assert_eq!(placed.overflow, 0.0);
    }

    #[test]
    fn a_menu_near_the_bottom_opens_above() {
        let anchor = anchor_at(500.0);
        let placed = place(anchor, vec2(120.0, 200.0), SCREEN, 1.0);
        assert_eq!(placed.side, Side::Above);
        assert!(placed.body.max.y < placed.tip.y && placed.tip.y < anchor.min.y);
        assert_eq!(placed.body.height(), 200.0);
    }

    #[test]
    fn a_menu_too_tall_for_either_side_takes_the_roomier_and_scrolls() {
        let placed = place(anchor_at(400.0), vec2(120.0, 900.0), SCREEN, 1.0);
        assert_eq!(placed.side, Side::Above);
        assert!(placed.body.min.y >= SCREEN.min.y);
        assert!((placed.overflow - (900.0 - placed.body.height())).abs() < 1e-3);
        assert!(placed.overflow > 0.0);
    }

    #[test]
    fn a_menu_stays_on_screen_sideways_and_points_at_its_anchor() {
        let anchor = Rect::from_min_size(pos2(740.0, 100.0), vec2(30.0, 22.0));
        let placed = place(anchor, vec2(200.0, 100.0), SCREEN, 1.0);
        assert_eq!(placed.body.max.x, SCREEN.max.x);
        assert_eq!(placed.tip.x, anchor.center().x);

        // The pointer stops short of the corner.
        let edge = Rect::from_min_size(pos2(790.0, 100.0), vec2(10.0, 22.0));
        let placed = place(edge, vec2(200.0, 100.0), SCREEN, 1.0);
        assert_eq!(placed.tip.x, placed.body.max.x - 2.0 * POINTER);
    }

    #[test]
    fn a_menu_fits_its_widest_option_and_the_check_column_but_never_less_than_its_slot() {
        let a = size(50.0, 3, 10.0, 1.0);
        assert_eq!(a.x, TEXT_INSET + 50.0 + CHECK_WIDTH);
        assert_eq!(a.y, 2.0 * INSET + 3.0 * ROW_HEIGHT);
        assert_eq!(size(50.0, 3, 300.0, 1.0).x, 300.0, "never narrower than its slot");
    }

    #[test]
    fn rows_are_found_where_they_are_drawn_even_scrolled() {
        let body = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0));
        for scroll in [0.0, 30.0] {
            for index in 0..5 {
                let center = row(body, index, scroll, 1.5).center();
                let expected = body.contains(center).then_some(index);
                assert_eq!(row_at(body, center, 5, scroll, 1.5), expected);
            }
        }
        assert_eq!(row_at(body, pos2(50.0, 2.0), 5, 0.0, 1.0), None, "the inset is no row");
    }
}
