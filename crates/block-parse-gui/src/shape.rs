//! Block outlines in canvas units, from jellycell's notch-and-tab geometry.
//! Fills are split into convex pieces: epaint fills a closed path as a
//! triangle fan, which would fill a concave notch back in.

use egui::{Pos2, Rect, pos2};

use crate::layout::{Section, StackForm};

pub const NOTCH_INSET: f32 = 14.0;
/// At the top edge; the trapezoid's wide end.
pub const NOTCH_WIDTH: f32 = 24.0;
/// Also how far the tab hangs below.
pub const NOTCH_DEPTH: f32 = 8.0;
pub const NOTCH_SLANT: f32 = 5.0;
pub const HAT_RISE: f32 = 16.0;
pub const HAT_WIDTH: f32 = 72.0;
/// Also the indent of a branch's blocks.
pub const ARM_WIDTH: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopEdge {
    Notched,
    Hat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomEdge {
    Tab,
    /// A cap.
    Flat,
}

pub type ConvexPiece = Vec<Pos2>;

/// Clockwise from the top-left, y down.
pub fn stack_outline(rect: Rect, form: &StackForm) -> Vec<Pos2> {
    let (x0, x1) = (rect.min.x, rect.max.x);
    let inner = x0 + ARM_WIDTH;
    let top = first_row_top(form);

    let mut points = top_edge(x0, x1 - x0, top, form.top);
    for (index, section) in form.sections.iter().enumerate() {
        match *section {
            Section::Row { bottom, .. } => {
                points.push(pos2(x1, bottom));
                if matches!(form.sections.get(index + 1), Some(Section::Branch { .. })) {
                    points.extend(tab_leftward(inner, bottom));
                    points.push(pos2(inner, bottom));
                }
            }
            Section::Branch { bottom, .. } => {
                points.push(pos2(inner, bottom));
                points.push(pos2(x1, bottom));
            }
        }
    }
    let bottom = last_bottom(form);
    if form.bottom == BottomEdge::Tab {
        points.extend(tab_leftward(x0, bottom));
    }
    points.push(pos2(x0, bottom));
    points
}

pub fn stack_fill(rect: Rect, form: &StackForm) -> Vec<ConvexPiece> {
    let (x0, x1) = (rect.min.x, rect.max.x);
    let inner = x0 + ARM_WIDTH;
    let quad = |left: f32, top: f32, right: f32, bottom: f32| {
        vec![pos2(left, top), pos2(right, top), pos2(right, bottom), pos2(left, bottom)]
    };

    let mut pieces = Vec::new();
    for (index, section) in form.sections.iter().enumerate() {
        match *section {
            Section::Row { top, bottom } => {
                let body_top = if index == 0 && form.top == TopEdge::Notched {
                    let n = Notch::at(x0, top);
                    pieces.push(vec![
                        pos2(x0, top),
                        pos2(n.wide.0, top),
                        pos2(n.narrow.0, n.floor),
                        pos2(x0, n.floor),
                    ]);
                    pieces.push(vec![
                        pos2(n.wide.1, top),
                        pos2(x1, top),
                        pos2(x1, n.floor),
                        pos2(n.narrow.1, n.floor),
                    ]);
                    n.floor
                } else {
                    top
                };
                pieces.push(quad(x0, body_top, x1, bottom));
                if matches!(form.sections.get(index + 1), Some(Section::Branch { .. })) {
                    pieces.push(tab_leftward(inner, bottom));
                }
            }
            Section::Branch { top, bottom } => pieces.push(quad(x0, top, inner, bottom)),
        }
    }
    if form.top == TopEdge::Hat {
        let mut bump = hat_arc(x0, first_row_top(form));
        bump.reverse();
        pieces.push(bump);
    }
    if form.bottom == BottomEdge::Tab {
        pieces.push(tab_leftward(x0, last_bottom(form)));
    }
    pieces
}

/// A block's top edge from `x0`, dipping through the notch or rising over the
/// hat. The snap highlight traces this so it previews the real join.
pub fn top_edge(x0: f32, width: f32, y: f32, top: TopEdge) -> Vec<Pos2> {
    let x1 = x0 + width;
    let mut points = match top {
        TopEdge::Notched => {
            let n = Notch::at(x0, y);
            vec![
                pos2(x0, y),
                pos2(n.wide.0, y),
                pos2(n.narrow.0, n.floor),
                pos2(n.narrow.1, n.floor),
                pos2(n.wide.1, y),
            ]
        }
        TopEdge::Hat => hat_arc(x0, y),
    };
    points.push(pos2(x1, y));
    points
}

/// Pointed beside a first row `head` tall; below it the sides run straight
/// down to corners cut at the same angle. One row (`head` the full height)
/// is a plain hexagon.
pub fn hexagon(rect: Rect, head: f32) -> Vec<Pos2> {
    let half = (head / 2.0).min(rect.height() / 2.0);
    let mid = rect.min.y + half;
    let low = rect.max.y - half;
    let mut points = vec![
        pos2(rect.min.x, mid),
        pos2(rect.min.x + half, rect.min.y),
        pos2(rect.max.x - half, rect.min.y),
        pos2(rect.max.x, mid),
    ];
    if low > mid {
        points.push(pos2(rect.max.x, low));
    }
    points.extend([pos2(rect.max.x - half, rect.max.y), pos2(rect.min.x + half, rect.max.y)]);
    if low > mid {
        points.push(pos2(rect.min.x, low));
    }
    points
}

/// A menu body with a pointer out to `tip`, from the top edge when `tip` is
/// above the body and the bottom otherwise. Clockwise from the top-left.
pub fn callout_outline(body: Rect, tip: Pos2, half_base: f32) -> Vec<Pos2> {
    let (left, right) = (tip.x - half_base, tip.x + half_base);
    let [tl, tr, br, bl] = corners(body);
    if tip.y < body.min.y {
        vec![tl, pos2(left, body.min.y), tip, pos2(right, body.min.y), tr, br, bl]
    } else {
        vec![tl, tr, br, pos2(right, body.max.y), tip, pos2(left, body.max.y), bl]
    }
}

pub fn callout_fill(body: Rect, tip: Pos2, half_base: f32) -> Vec<ConvexPiece> {
    let edge = if tip.y < body.min.y { body.min.y } else { body.max.y };
    vec![
        corners(body).to_vec(),
        vec![pos2(tip.x - half_base, edge), tip, pos2(tip.x + half_base, edge)],
    ]
}

fn corners(rect: Rect) -> [Pos2; 4] {
    [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
}

/// Shared by outline and fill so the two cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Notch {
    floor: f32,
    /// x of the wide end, left and right.
    wide: (f32, f32),
    narrow: (f32, f32),
}

impl Notch {
    fn at(x0: f32, top: f32) -> Self {
        let left = x0 + NOTCH_INSET;
        Self {
            floor: top + NOTCH_DEPTH,
            wide: (left, left + NOTCH_WIDTH),
            narrow: (left + NOTCH_SLANT, left + NOTCH_WIDTH - NOTCH_SLANT),
        }
    }
}

/// The tab under an edge at `y`, right to left, for a block whose left edge
/// is `x0`: exactly the notch of a block placed below at `x0`.
fn tab_leftward(x0: f32, y: f32) -> Vec<Pos2> {
    let n = Notch::at(x0, y);
    vec![
        pos2(n.wide.1, y),
        pos2(n.narrow.1, n.floor),
        pos2(n.narrow.0, n.floor),
        pos2(n.wide.0, y),
    ]
}

/// Left to right over the top of a hat, ending back on `y`.
fn hat_arc(x0: f32, y: f32) -> Vec<Pos2> {
    const SEGMENTS: usize = 12;
    (0..=SEGMENTS)
        .map(|i| {
            let t = std::f32::consts::PI * i as f32 / SEGMENTS as f32;
            pos2(
                x0 + HAT_WIDTH * (1.0 - t.cos()) / 2.0,
                y - HAT_RISE * t.sin(),
            )
        })
        .collect()
}

fn first_row_top(form: &StackForm) -> f32 {
    match form.sections.first() {
        Some(Section::Row { top, .. } | Section::Branch { top, .. }) => *top,
        None => 0.0,
    }
}

fn last_bottom(form: &StackForm) -> f32 {
    match form.sections.last() {
        Some(Section::Row { bottom, .. } | Section::Branch { bottom, .. }) => *bottom,
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    /// Twice the signed area.
    fn shoelace(points: &[Pos2]) -> f32 {
        (0..points.len())
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum()
    }

    fn forms() -> Vec<StackForm> {
        let row = |top, bottom| Section::Row { top, bottom };
        let branch = |top, bottom| Section::Branch { top, bottom };
        vec![
            StackForm {
                top: TopEdge::Notched,
                bottom: BottomEdge::Tab,
                sections: vec![row(0.0, 40.0)],
            },
            StackForm {
                top: TopEdge::Hat,
                bottom: BottomEdge::Tab,
                sections: vec![row(HAT_RISE, HAT_RISE + 40.0)],
            },
            StackForm {
                top: TopEdge::Notched,
                bottom: BottomEdge::Flat,
                sections: vec![row(0.0, 40.0)],
            },
            // An if/else.
            StackForm {
                top: TopEdge::Notched,
                bottom: BottomEdge::Tab,
                sections: vec![
                    row(0.0, 40.0),
                    branch(40.0, 80.0),
                    row(80.0, 110.0),
                    branch(110.0, 134.0),
                    row(134.0, 154.0),
                ],
            },
        ]
    }

    fn rect(form: &StackForm) -> Rect {
        Rect::from_min_max(pos2(30.0, 0.0), pos2(190.0, last_bottom(form)))
    }

    #[test]
    fn every_fill_piece_is_convex() {
        for form in forms() {
            for (index, piece) in stack_fill(rect(&form), &form).iter().enumerate() {
                let n = piece.len();
                let signs: Vec<f32> = (0..n)
                    .map(|i| {
                        let a = piece[(i + 1) % n] - piece[i];
                        let b = piece[(i + 2) % n] - piece[(i + 1) % n];
                        a.x * b.y - a.y * b.x
                    })
                    .filter(|turn| turn.abs() > 1e-4)
                    .map(f32::signum)
                    .collect();
                assert!(
                    signs.windows(2).all(|pair| pair[0] == pair[1]),
                    "{form:?}: piece {index} is not convex: {piece:?}"
                );
            }
        }
    }

    #[test]
    fn the_fill_covers_the_outline_exactly() {
        // Equal area means the pieces neither overlap nor leave a gap.
        for form in forms() {
            let outline = shoelace(&stack_outline(rect(&form), &form)).abs();
            let pieces: f32 = stack_fill(rect(&form), &form)
                .iter()
                .map(|piece| shoelace(piece).abs())
                .sum();
            assert!((outline - pieces).abs() < 0.05, "{form:?}: {outline} vs {pieces}");
        }
    }

    #[test]
    fn a_callout_fill_covers_its_outline_either_way_up() {
        let body = Rect::from_min_size(pos2(10.0, 20.0), vec2(80.0, 50.0));
        for tip in [pos2(40.0, 12.0), pos2(40.0, 78.0)] {
            let outline = shoelace(&callout_outline(body, tip, 7.0)).abs();
            let pieces: f32 = callout_fill(body, tip, 7.0)
                .iter()
                .map(|piece| shoelace(piece).abs())
                .sum();
            assert!((outline - pieces).abs() < 0.05, "{tip:?}: {outline} vs {pieces}");
            assert!(outline > shoelace(&corners(body)).abs(), "{tip:?}: the pointer adds area");
        }
    }

    #[test]
    fn a_tab_is_exactly_the_notch_below_it() {
        let tab = tab_leftward(30.0, 40.0);
        let notch = top_edge(30.0, 160.0, 40.0, TopEdge::Notched);
        let mut notch: Vec<Pos2> = notch[1..5].to_vec();
        notch.reverse();
        for (a, b) in tab.iter().zip(&notch) {
            assert!(a.distance(*b) < 1e-4, "{tab:?} vs {notch:?}");
        }
    }

    #[test]
    fn hexagons_are_as_tall_as_their_rect() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(60.0, 22.0));
        let points = hexagon(rect, 22.0);
        assert_eq!(points.len(), 6);
        assert_eq!(points[1].y, 0.0);
        assert_eq!(points[4].y, 22.0);
    }

    #[test]
    fn a_taller_hexagon_keeps_its_points_beside_the_first_row() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(60.0, 70.0));
        let points = hexagon(rect, 22.0);
        assert_eq!(points.len(), 8);
        assert_eq!((points[0].y, points[3].y), (11.0, 11.0));
        assert_eq!((points[4], points[7].y), (pos2(60.0, 59.0), 59.0));
    }
}
