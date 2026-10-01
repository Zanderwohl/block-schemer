//! Programs to images, rendered offscreen with wgpu. Panics without a wgpu
//! adapter.

use block_parse::host::Overlay;
use block_parse::program::Program;
use block_parse::Language;
use egui::{LayerId, Vec2, vec2};
use egui_kittest::Harness;
pub use image::RgbaImage;

use crate::color::Swatches;
use crate::editor::EguiMeasure;
use crate::layout::{Layout, Scene};
use crate::paint::{self, Transform};
use crate::theme::Theme;

/// Canvas units around the program's blocks.
const MARGIN: f32 = 24.0;
/// egui-wgpu's device limit; a larger texture panics inside wgpu.
const MAX_PIXELS: f32 = 8192.0;

/// `program` cropped to its blocks, at `scale` pixels per canvas unit. The
/// overlay's bubbles stay inside the margin where they fit.
pub fn program(
    language: &Language,
    program: &Program,
    overlay: &Overlay,
    theme: &Theme,
    scale: f32,
) -> Result<RgbaImage, String> {
    render(language, overlay, theme, scale, |layout| layout.program(program))
}

/// Every block in the language, laid out by [`Layout::grid`].
pub fn grid(language: &Language, theme: &Theme, scale: f32) -> Result<RgbaImage, String> {
    render(language, &Overlay::default(), theme, scale, |layout| layout.program(&layout.grid()))
}

/// Text is measured by the harness's own fonts, so the scene is laid out
/// inside a frame and the harness resized to fit before the one that renders.
fn render(
    language: &Language,
    overlay: &Overlay,
    theme: &Theme,
    scale: f32,
    scene: impl Fn(&Layout) -> Scene,
) -> Result<RgbaImage, String> {
    if !(scale > 0.0 && scale.is_finite()) {
        return Err(format!("scale must be positive, not {scale}"));
    }
    let swatches = Swatches::resolve(language, &theme.swatch);
    let mut harness = Harness::builder().with_pixels_per_point(scale).wgpu().build_ui_state(
        |ui, size: &mut Vec2| {
            let ctx = ui.ctx().clone();
            let measure = EguiMeasure(&ctx);
            let scene = scene(&Layout {
                language,
                measure: &measure,
                swatches: &swatches,
                editing: None,
                validate: true,
                lifted: None,
            });
            let painter = ctx.layer_painter(LayerId::background());
            let (bounds, bubbles) = if scene.blocks.is_empty() {
                (egui::Rect::ZERO, Vec::new())
            } else {
                let bubbles = paint::place_bubbles(&painter, &scene, 1.0, theme, overlay, scene.bounds.expand(MARGIN));
                let bounds = bubbles.iter().fold(scene.bounds, |bounds, (bubble, _)| bounds.union(bubble.body));
                (bounds, bubbles)
            };
            *size = bounds.size() + vec2(2.0, 2.0) * MARGIN;
            let t = Transform {
                origin: (vec2(MARGIN, MARGIN) - bounds.min.to_vec2()).to_pos2(),
                zoom: 1.0,
            };
            painter.rect_filled(ctx.content_rect(), 0.0, theme.canvas);
            paint::scene(&painter, &scene, t, theme, false, overlay);
            paint::error_tags(&painter, &scene, t, theme);
            paint::markers(&painter, &scene, t, theme, overlay);
            paint::bubbles(&painter, bubbles, t, theme);
        },
        Vec2::ZERO,
    );
    let size = *harness.state();
    let pixels = size * scale;
    if pixels.max_elem() > MAX_PIXELS {
        return Err(format!(
            "the image would be {:.0}×{:.0} px, over {MAX_PIXELS} px: lower the scale",
            pixels.x, pixels.y
        ));
    }
    harness.set_size(size);
    harness.step();
    harness.render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_parse::Validators;

    #[test]
    fn grid_image_is_the_scene_plus_margins_on_the_canvas_color() {
        let language = Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap();
        let theme = Theme::default();
        let image = grid(&language, &theme, 1.0).unwrap();

        let ctx = egui::Context::default();
        // Loads the fonts the harness measures with.
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let swatches = Swatches::resolve(&language, &theme.swatch);
        let measure = EguiMeasure(&ctx);
        let layout = Layout {
            language: &language,
            measure: &measure,
            swatches: &swatches,
            editing: None,
            validate: true,
            lifted: None,
        };
        let size = layout.program(&layout.grid()).bounds.size() + vec2(2.0, 2.0) * MARGIN;
        assert_eq!((image.width(), image.height()), (size.x.round() as u32, size.y.round() as u32));
        let corner = image.get_pixel(0, 0).0;
        assert_eq!(corner, theme.canvas.to_array());
    }

    #[test]
    fn oversized_or_unscaled_images_are_errors() {
        let language = Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap();
        let theme = Theme::default();
        assert!(grid(&language, &theme, 0.0).is_err());
        assert!(grid(&language, &theme, f32::NAN).is_err());
        assert!(grid(&language, &theme, 100.0).unwrap_err().contains("lower the scale"));
    }
}
