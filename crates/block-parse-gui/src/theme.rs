//! Colors that do not come from the language.

use block_parse::HighlightStyle;
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
    /// The edges between the canvas and its panels.
    pub divider: Color32,
    pub snap: Color32,
    /// Drawn under accents so they never depend on contrast with the block,
    /// and as the shadow of a choice's menu.
    pub halo: Color32,
    pub breakpoint: Color32,
    pub selected: Color32,
    pub related: Color32,
    pub active: Color32,
    pub dispatched: Color32,
    /// For `HighlightStyle::Custom`; an index past the end uses `selected`.
    pub custom: Vec<Color32>,
    pub error: Color32,
    pub warning: Color32,
    pub literal_fill: Color32,
    pub literal_ink: Color32,
    /// A blank field's hint.
    pub placeholder: Color32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            swatch: SwatchRecipe::default(),
            canvas: Color32::from_rgb(0x16, 0x18, 0x1d),
            grid: Color32::from_rgb(0x2a, 0x2d, 0x35),
            palette: Color32::from_rgb(0x1f, 0x22, 0x29),
            palette_heading: Color32::from_gray(0xa0),
            divider: Color32::from_rgb(0x34, 0x38, 0x42),
            snap: Color32::WHITE,
            halo: Color32::from_black_alpha(150),
            breakpoint: Color32::from_rgb(0xe5, 0x48, 0x4d),
            selected: Color32::WHITE,
            related: Color32::from_rgb(0xf2, 0xc4, 0x3d),
            active: Color32::from_rgb(0x4a, 0xe0, 0x6a),
            dispatched: Color32::from_rgb(0xff, 0xd8, 0x1a),
            custom: Vec::new(),
            error: Color32::from_rgb(0xe5, 0x48, 0x4d),
            warning: Color32::from_rgb(0xf5, 0xa5, 0x24),
            literal_fill: Color32::WHITE,
            literal_ink: Color32::from_rgb(0x1d, 0x1f, 0x24),
            placeholder: Color32::from_gray(0x9a),
        }
    }
}

impl Theme {
    pub fn highlight(&self, style: HighlightStyle) -> Color32 {
        match style {
            HighlightStyle::Selected => self.selected,
            HighlightStyle::Related => self.related,
            HighlightStyle::Active => self.active,
            HighlightStyle::Dispatched => self.dispatched,
            HighlightStyle::Custom(index) => self
                .custom
                .get(usize::from(index))
                .copied()
                .unwrap_or(self.selected),
        }
    }
}
