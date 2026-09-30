//! Layout is done at zoom 1 and scaled when drawn, fonts included. Measuring
//! at the drawn size would make stacks re-flow as you zoom.

use egui::{Pos2, Rect, Vec2, vec2};

use crate::paint::Transform;

pub const MIN_ZOOM: f32 = 0.4;
pub const MAX_ZOOM: f32 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Screen offset of the canvas origin from the canvas area's top-left.
    pub pan: Vec2,
    /// Screen pixels per canvas unit.
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pan: vec2(24.0, 24.0),
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn transform(&self, area: Rect) -> Transform {
        Transform {
            origin: area.min + self.pan,
            zoom: self.zoom,
        }
    }

    /// Zooms by `factor`, keeping the canvas point under `pointer` still.
    pub fn zoom_about(&mut self, area: Rect, pointer: Pos2, factor: f32) {
        let before = self.transform(area).canvas(pointer);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = pointer - area.min - before.to_vec2() * self.zoom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let area = Rect::from_min_size(pos2(200.0, 30.0), vec2(800.0, 600.0));
        let mut view = View::default();
        let pointer = pos2(530.0, 310.0);
        let before = view.transform(area).canvas(pointer);

        view.zoom_about(area, pointer, 1.7);
        let after = view.transform(area).canvas(pointer);
        assert!(before.distance(after) < 1e-3, "{before:?} moved to {after:?}");
        assert_eq!(view.zoom, 1.7);
    }
}
