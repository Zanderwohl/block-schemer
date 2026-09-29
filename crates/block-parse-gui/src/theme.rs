//! Colours that do not come from the language.

use egui::Color32;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub canvas: Color32,
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
