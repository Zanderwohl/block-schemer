//! Block colors. Each category gives an OKLCH hue; every role is a step from
//! it in lightness and chroma, so categories read at the same perceived
//! lightness. Resolved to sRGB, reducing chroma where a step leaves the gamut.

use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swatch {
    pub fill: Color32,
    pub edge: Color32,
    pub shadow: Color32,
    pub highlight: Color32,
    /// Inactive blocks: same lightness as `fill`, so they keep their weight.
    pub muted: Color32,
    /// Black or white, whichever contrasts with `fill`.
    pub ink: Color32,
}

/// How a hue becomes a [`Swatch`].
#[derive(Debug, Clone, PartialEq)]
pub struct SwatchRecipe {
    /// For categories that give none.
    pub chroma: f32,
    pub lightness: f32,
    /// For uncategorized blocks.
    pub neutral_hue: f32,
    pub edge: Step,
    pub shadow: Step,
    pub highlight: Step,
    pub muted: Step,
}

/// A move from the base color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// Added to OKLCH lightness.
    pub lightness: f32,
    /// Multiplies OKLCH chroma.
    pub chroma: f32,
}

/// One per category, in the language's category order.
#[derive(Debug, Clone, PartialEq)]
pub struct Swatches {
    pub categories: Vec<Swatch>,
    pub uncategorized: Swatch,
}
