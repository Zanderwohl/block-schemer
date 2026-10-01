//! Speech bubbles beside blocks, in canvas units: where one goes, and its
//! outline with a tail pointing back at its block.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::shape::ConvexPiece;

pub const RADIUS: f32 = 8.0;
pub const PADDING: Vec2 = vec2(10.0, 7.0);
/// Between a bubble and its block, so the tail has room.
pub const GAP: f32 = 16.0;
const TAIL_HALF_BASE: f32 = 6.0;
/// The tail stops short of the block.
const TIP_GAP: f32 = 2.0;
/// Arc segments per rounded corner.
const SEGMENTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bubble {
    pub body: Rect,
    pub tail: Tail,
}

/// The base sits on one straight side of the body, in outline order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tail {
    pub side: Side,
    pub base: [Pos2; 2],
    pub tip: Pos2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// Points at `target`, a block's first row, and keeps off `block`, all of it.
/// The scoring is in `documentation/02-bubbles.md`.
pub fn place(target: Rect, block: Rect, size: Vec2, obstacles: &[Rect], visible: Rect) -> Bubble {
    let body = candidates(target, size)
        .into_iter()
        .enumerate()
        .map(|(rank, body)| {
            let outside = body.area() - overlap(body, visible);
            let covered: f32 = obstacles.iter().map(|o| overlap(body, *o)).sum();
            // A bubble cut off cannot be read; one over its block hides it.
            let penalty = overlap(body, block) * 1000.0 + outside * 4.0 + covered;
            (penalty + rank as f32 * 0.01, body)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, body)| body)
        .expect("there is always a candidate");
    Bubble {
        body,
        tail: tail(body, target),
    }
}

/// In order of preference.
fn candidates(target: Rect, size: Vec2) -> Vec<Rect> {
    let at = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), size);
    let right = target.max.x + GAP;
    let left = target.min.x - GAP - size.x;
    let above = target.min.y - GAP - size.y;
    let below = target.max.y + GAP;
    let rows = [target.min.y, target.center().y - size.y / 2.0, target.max.y - size.y];
    let columns = [target.min.x, target.center().x - size.x / 2.0, target.max.x - size.x];
    // Diagonals sit closer in, so their tails are no longer than the rest.
    let diagonal = GAP * std::f32::consts::FRAC_1_SQRT_2;
    let mut spots = Vec::new();
    spots.extend(rows.map(|y| at(right, y)));
    spots.extend(columns.map(|x| at(x, above)));
    spots.push(at(target.max.x + diagonal, target.min.y - diagonal - size.y));
    spots.extend(columns.map(|x| at(x, below)));
    spots.push(at(target.max.x + diagonal, target.max.y + diagonal));
    spots.extend(rows.map(|y| at(left, y)));
    spots.push(at(target.min.x - diagonal - size.x, target.min.y - diagonal - size.y));
    spots.push(at(target.min.x - diagonal - size.x, target.max.y + diagonal));
    spots
}

fn overlap(a: Rect, b: Rect) -> f32 {
    let i = a.intersect(b);
    if i.is_positive() { i.area() } else { 0.0 }
}

/// From the point on `body`'s edge nearest `target` to the point on `target`
/// nearest that. The base stays on a straight side, clear of the corners.
fn tail(body: Rect, target: Rect) -> Tail {
    let gap_x = (target.min.x - body.max.x).max(body.min.x - target.max.x);
    let gap_y = (target.min.y - body.max.y).max(body.min.y - target.max.y);
    let side = if gap_x >= gap_y {
        if target.center().x > body.center().x { Side::Right } else { Side::Left }
    } else if target.center().y > body.center().y {
        Side::Bottom
    } else {
        Side::Top
    };
    let inset = RADIUS + TAIL_HALF_BASE;
    let along = |lo: f32, hi: f32, t_lo: f32, t_hi: f32| {
        // Midway along the shared span, else the end nearest the target.
        let near = ((lo.max(t_lo) + hi.min(t_hi)) / 2.0).clamp(t_lo, t_hi);
        if hi - lo < 2.0 * inset { (lo + hi) / 2.0 } else { near.clamp(lo + inset, hi - inset) }
    };
    let (anchor, base) = match side {
        Side::Top | Side::Bottom => {
            let x = along(body.min.x, body.max.x, target.min.x, target.max.x);
            let y = if side == Side::Top { body.min.y } else { body.max.y };
            let base = [pos2(x - TAIL_HALF_BASE, y), pos2(x + TAIL_HALF_BASE, y)];
            (pos2(x, y), if side == Side::Top { base } else { [base[1], base[0]] })
        }
        Side::Left | Side::Right => {
            let y = along(body.min.y, body.max.y, target.min.y, target.max.y);
            let x = if side == Side::Left { body.min.x } else { body.max.x };
            let base = [pos2(x, y - TAIL_HALF_BASE), pos2(x, y + TAIL_HALF_BASE)];
            (pos2(x, y), if side == Side::Right { base } else { [base[1], base[0]] })
        }
    };
    let nearest = target.clamp(anchor);
    let reach = nearest - anchor;
    let tip = anchor + reach * ((reach.length() - TIP_GAP).max(0.0) / reach.length().max(f32::EPSILON));
    Tail { side, base, tip }
}

impl Bubble {
    /// Clockwise from the top-left corner's start, y down.
    pub fn outline(&self) -> Vec<Pos2> {
        let mut points = Vec::new();
        for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
            points.extend(corner(self.body, side));
            if side == self.tail.side {
                points.extend([self.tail.base[0], self.tail.tip, self.tail.base[1]]);
            }
        }
        points
    }

    pub fn fill(&self) -> Vec<ConvexPiece> {
        let body = [Side::Top, Side::Right, Side::Bottom, Side::Left]
            .into_iter()
            .flat_map(|side| corner(self.body, side))
            .collect();
        vec![body, vec![self.tail.base[0], self.tail.tip, self.tail.base[1]]]
    }

    pub fn translate(self, by: Vec2) -> Self {
        Self {
            body: self.body.translate(by),
            tail: Tail {
                base: self.tail.base.map(|p| p + by),
                tip: self.tail.tip + by,
                ..self.tail
            },
        }
    }
}

/// The corner that starts `side` when going clockwise.
fn corner(rect: Rect, side: Side) -> Vec<Pos2> {
    let r = RADIUS.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let (center, start) = match side {
        Side::Top => (rect.left_top() + vec2(r, r), std::f32::consts::PI),
        Side::Right => (rect.right_top() + vec2(-r, r), -std::f32::consts::FRAC_PI_2),
        Side::Bottom => (rect.right_bottom() + vec2(-r, -r), 0.0),
        Side::Left => (rect.left_bottom() + vec2(r, -r), std::f32::consts::FRAC_PI_2),
    };
    (0..=SEGMENTS)
        .map(|i| {
            let angle = start + std::f32::consts::FRAC_PI_2 * i as f32 / SEGMENTS as f32;
            center + r * vec2(angle.cos(), angle.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Rect {
        Rect::from_min_size(pos2(100.0, 100.0), vec2(80.0, 30.0))
    }

    fn visible() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0))
    }

    #[test]
    fn with_room_everywhere_a_bubble_goes_right_of_its_block() {
        let bubble = place(target(), target(), vec2(60.0, 24.0), &[], visible());
        assert_eq!(bubble.body.min, pos2(180.0 + GAP, 100.0));
        assert_eq!(bubble.tail.side, Side::Left);
        assert!((bubble.tail.tip.x - (180.0 + TIP_GAP)).abs() < 1e-4);
    }

    #[test]
    fn a_bubble_never_covers_its_block_and_moves_off_obstacles_and_edges() {
        let size = vec2(60.0, 24.0);
        let right = Rect::from_min_size(pos2(185.0, 60.0), vec2(200.0, 100.0));
        let bubble = place(target(), target(), size, &[right], visible());
        assert!(!bubble.body.intersects(target()));
        assert!(!bubble.body.intersects(right));

        let edge = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 1000.0));
        let bubble = place(target(), target(), size, &[], edge);
        assert!(edge.contains_rect(bubble.body), "{:?}", bubble.body);
        assert!(!bubble.body.intersects(target()));
    }

    #[test]
    fn a_bubble_keeps_off_the_rest_of_a_tall_block_even_when_crowded() {
        let block = Rect::from_min_size(target().min, vec2(80.0, 200.0));
        let left = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 1000.0));
        let right = Rect::from_min_max(pos2(180.0, 0.0), pos2(1000.0, 1000.0));
        let bubble = place(target(), block, vec2(60.0, 24.0), &[left, right], visible());
        assert!(!bubble.body.intersects(block), "{:?}", bubble.body);
    }

    #[test]
    fn the_tail_leaves_the_side_facing_the_block_clear_of_the_corners() {
        let body = Rect::from_min_size(pos2(200.0, 20.0), vec2(60.0, 40.0));
        let tail = tail(body, target());
        assert_eq!(tail.side, Side::Bottom);
        for p in tail.base {
            assert_eq!(p.y, body.max.y);
            assert!(p.x >= body.min.x + RADIUS && p.x <= body.max.x - RADIUS);
        }
        assert!(target().expand(TIP_GAP + 1e-3).contains(tail.tip));
    }

    #[test]
    fn the_fill_covers_the_outline() {
        fn area(points: &[Pos2]) -> f32 {
            (0..points.len())
                .map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % points.len()]);
                    a.x * b.y - b.x * a.y
                })
                .sum::<f32>()
                .abs()
                / 2.0
        }
        for spot in candidates(target(), vec2(60.0, 40.0)) {
            let bubble = Bubble {
                body: spot,
                tail: tail(spot, target()),
            };
            let pieces: f32 = bubble.fill().iter().map(|p| area(p)).sum();
            assert!((area(&bubble.outline()) - pieces).abs() < 0.05, "{bubble:?}");
        }
    }
}
