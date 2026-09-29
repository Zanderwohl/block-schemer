//! Layout is done at zoom 1 and scaled when drawn, fonts included. Measuring
//! at the drawn size would make stacks re-flow as you zoom.

use egui::Vec2;

pub const MIN_ZOOM: f32 = 0.4;
pub const MAX_ZOOM: f32 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Screen offset of the canvas origin from the canvas area's top-left.
    pub pan: Vec2,
    /// Screen pixels per canvas unit.
    pub zoom: f32,
}
