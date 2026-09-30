//! Block colors. Each category gives an OKLCH hue; every role is a step from
//! it in lightness and chroma, so categories read at the same perceived
//! lightness. Resolved to sRGB, reducing chroma where a step leaves the gamut.

use block_parse::Language;
use egui::Color32;
use palette::convert::FromColorUnclamped;
use palette::{Clamp, IsWithinBounds, Oklch, Srgb};

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
    /// For uncategorized blocks, at a fraction of `chroma`.
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

impl Default for SwatchRecipe {
    fn default() -> Self {
        Self {
            chroma: 0.14,
            lightness: 0.66,
            neutral_hue: 260.0,
            edge: Step {
                lightness: -0.17,
                chroma: 1.0,
            },
            shadow: Step {
                lightness: -0.10,
                chroma: 0.9,
            },
            highlight: Step {
                lightness: 0.12,
                chroma: 0.7,
            },
            muted: Step {
                lightness: 0.0,
                chroma: 0.1,
            },
        }
    }
}

impl SwatchRecipe {
    pub fn swatch(&self, hue: f32, chroma: Option<f32>, lightness: Option<f32>) -> Swatch {
        let chroma = chroma.unwrap_or(self.chroma);
        let lightness = lightness.unwrap_or(self.lightness);
        let step = |step: Step| srgb(lightness + step.lightness, chroma * step.chroma, hue);
        Swatch {
            fill: srgb(lightness, chroma, hue),
            edge: step(self.edge),
            shadow: step(self.shadow),
            highlight: step(self.highlight),
            muted: step(self.muted),
            ink: if lightness > 0.75 {
                Color32::from_gray(24)
            } else {
                Color32::WHITE
            },
        }
    }
}

impl Swatches {
    pub fn resolve(language: &Language, recipe: &SwatchRecipe) -> Self {
        Self {
            categories: language
                .categories()
                .iter()
                .map(|category| {
                    let color = category.color;
                    recipe.swatch(color.hue, color.chroma, color.lightness)
                })
                .collect(),
            uncategorized: recipe.swatch(recipe.neutral_hue, Some(recipe.chroma * 0.15), None),
        }
    }
}

fn srgb(lightness: f32, chroma: f32, hue: f32) -> Color32 {
    let lightness = lightness.clamp(0.0, 1.0);
    let mut chroma = chroma.max(0.0);
    loop {
        let color = Srgb::from_color_unclamped(Oklch::new(lightness, chroma, hue));
        if color.is_within_bounds() || chroma < 1e-3 {
            let color: Srgb<u8> = color.clamp().into_format();
            return Color32::from_rgb(color.red, color.green, color.blue);
        }
        chroma *= 0.92;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_gamut_steps_lose_chroma_rather_than_hue() {
        // Very high chroma at a blue hue is far outside sRGB.
        let recipe = SwatchRecipe::default();
        let swatch = recipe.swatch(265.0, Some(0.37), None);
        let [r, g, b, _] = swatch.fill.to_array();
        assert!(b > r && b > g, "still blue: {:?}", swatch.fill);
    }

    #[test]
    fn roles_step_lightness_the_way_their_names_say() {
        let swatch = SwatchRecipe::default().swatch(145.0, None, None);
        let brightness = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(brightness(swatch.edge) < brightness(swatch.fill));
        assert!(brightness(swatch.shadow) < brightness(swatch.fill));
        assert!(brightness(swatch.highlight) > brightness(swatch.fill));
    }
}
