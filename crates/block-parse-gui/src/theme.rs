//! Colors that do not come from the language.

use egui::Color32;

use crate::color::SwatchRecipe;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub swatch: SwatchRecipe,
    pub canvas: Color32,
    /// Dots marking canvas units, so panning reads as movement.
    pub grid: Color32,
    pub palette: Color32,
    pub palette_heading: Color32,
    pub snap: Color32,
    /// Drawn under accents so they never depend on contrast with the block.
    pub halo: Color32,
    pub breakpoint: Color32,
    pub paused: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub literal_fill: Color32,
    pub literal_ink: Color32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            swatch: SwatchRecipe::default(),
            canvas: Color32::from_rgb(0x16, 0x18, 0x1d),
            grid: Color32::from_rgb(0x2a, 0x2d, 0x35),
            palette: Color32::from_rgb(0x1f, 0x22, 0x29),
            palette_heading: Color32::from_gray(0xa0),
            snap: Color32::WHITE,
            halo: Color32::from_black_alpha(150),
            breakpoint: Color32::from_rgb(0xe5, 0x48, 0x4d),
            paused: Color32::from_rgb(0x4a, 0xe0, 0x6a),
            error: Color32::from_rgb(0xe5, 0x48, 0x4d),
            warning: Color32::from_rgb(0xf5, 0xa5, 0x24),
            literal_fill: Color32::WHITE,
            literal_ink: Color32::from_rgb(0x1d, 0x1f, 0x24),
        }
    }
}
