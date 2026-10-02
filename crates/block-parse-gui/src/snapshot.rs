//! Programs to images, rendered offscreen with wgpu. Panics without a wgpu
//! adapter.

use std::collections::HashMap;
use std::path::Path;

use block_parse::host::Overlay;
use block_parse::program::{BlockId, Program};
use block_parse::Language;
use egui::{LayerId, Vec2};
use egui_kittest::Harness;
pub use image::RgbaImage;

use crate::color::Swatches;
use crate::editor::EguiMeasure;
use crate::layout::{Layout, Scene};
use crate::paint::{self, Transform};
use crate::theme::Theme;

/// Canvas units around the program's blocks.
const MARGIN: f32 = 24.0;
/// Kept between a bubble and the image's edge, for its outline and shadow.
const BUBBLE_EDGE: f32 = 4.0;
/// egui-wgpu's device limit; a larger texture panics inside wgpu.
const MAX_PIXELS: f32 = 8192.0;

/// `program` cropped to its blocks, at `scale` pixels per canvas unit. The
/// overlay's bubbles stay inside the margin where they fit; the image grows
/// for those that do not.
pub fn program(
    language: &Language,
    program: &Program,
    overlay: &Overlay,
    theme: &Theme,
    scale: f32,
) -> Result<RgbaImage, String> {
    let declarers = Layout::declarers(language, program);
    render(language, overlay, theme, scale, declarers, |layout| layout.program(program))
}

/// Load warnings, such as another language's name, do not stop it.
pub fn program_file(language: &Language, path: &Path, theme: &Theme, scale: f32) -> Result<RgbaImage, String> {
    let (loaded, _) = Program::load(path, language).map_err(|error| format!("{}: {error}", path.display()))?;
    program(language, &loaded, &Overlay::default(), theme, scale)
}

/// Writes `image` to `path` as a PNG, whatever its extension.
pub fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Every block in the language, or only those with a tag in `tags`, laid
/// out by [`Layout::grid`].
pub fn grid(language: &Language, tags: Option<&[String]>, theme: &Theme, scale: f32) -> Result<RgbaImage, String> {
    render(language, &Overlay::default(), theme, scale, Default::default(), |layout| {
        layout.program(&layout.grid(tags))
    })
}

/// Text is measured by the harness's own fonts, so the scene is laid out
/// inside a frame and the harness resized to fit before the one that renders.
fn render(
    language: &Language,
    overlay: &Overlay,
    theme: &Theme,
    scale: f32,
    declarers: HashMap<BlockId, String>,
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
                declarers: declarers.clone(),
            });
            let painter = ctx.layer_painter(LayerId::background());
            let (frame, bubbles) = if scene.blocks.is_empty() {
                (egui::Rect::ZERO.expand(MARGIN), Vec::new())
            } else {
                let frame = scene.bounds.expand(MARGIN);
                let visible = frame.shrink(BUBBLE_EDGE);
                let bubbles = paint::place_bubbles(&painter, &scene, 1.0, theme, overlay, visible);
                let frame = bubbles
                    .iter()
                    .fold(frame, |frame, (bubble, _)| frame.union(bubble.body.expand(BUBBLE_EDGE)));
                (frame, bubbles)
            };
            *size = frame.size();
            let t = Transform {
                origin: (-frame.min.to_vec2()).to_pos2(),
                zoom: 1.0,
            };
            painter.rect_filled(ctx.content_rect(), 0.0, theme.canvas);
            paint::scene(&painter, &scene, t, theme, false, overlay);
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
    use egui::vec2;

    #[test]
    fn grid_image_is_the_scene_plus_margins_on_the_canvas_color() {
        let language = Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap();
        let theme = Theme::default();
        let image = grid(&language, None, &theme, 1.0).unwrap();

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
            declarers: Default::default(),
        };
        let size = layout.program(&layout.grid(None)).bounds.size() + vec2(2.0, 2.0) * MARGIN;
        assert_eq!((image.width(), image.height()), (size.x.round() as u32, size.y.round() as u32));
        let corner = image.get_pixel(0, 0).0;
        assert_eq!(corner, theme.canvas.to_array());
    }

    #[test]
    fn a_bubble_grows_the_image_only_when_the_margin_cannot_hold_it() {
        let language = Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap();
        let theme = Theme::default();
        let mut program = Program::new(&language);
        let mut ids = Vec::new();
        for x in [0.0, 300.0] {
            let block = program.instantiate(&language, "not").unwrap();
            ids.push(block.id);
            program.stacks.push(block_parse::Stack {
                pos: [x, 0.0],
                blocks: vec![block],
            });
        }
        let size = |program: &Program, overlay: &Overlay| {
            let image = super::program(&language, program, overlay, &theme, 1.0).unwrap();
            (image.width(), image.height())
        };

        // Between the two blocks there is room.
        let mut between = Overlay::default();
        between.bubbles.insert(ids[0], "ok".into());
        assert_eq!(size(&program, &between), size(&program, &Overlay::default()));

        program.stacks.truncate(1);
        let plain = size(&program, &Overlay::default());
        let grown = size(&program, &between);
        assert!(grown.0 > plain.0 || grown.1 > plain.1, "{grown:?} vs {plain:?}");
    }

    #[test]
    fn oversized_or_unscaled_images_are_errors() {
        let language = Language::from_ron(
            include_str!("../../../examples/languages/tiny.ron"),
            &Validators::new(),
        )
        .unwrap();
        let theme = Theme::default();
        assert!(grid(&language, None, &theme, 0.0).is_err());
        assert!(grid(&language, None, &theme, f32::NAN).is_err());
        assert!(grid(&language, None, &theme, 100.0).unwrap_err().contains("lower the scale"));
    }
}
