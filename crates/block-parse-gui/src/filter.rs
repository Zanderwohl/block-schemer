//! The palette's tag filter: an All box, then one per tag, in two columns
//! above the blocks. A block shows when any of its tags is checked, or All
//! is. Only languages that give `checked_tags` have one.

use block_parse::Language;
use egui::{Align, Color32, FontId, Rect, RichText, Stroke, TextWrapMode, UiBuilder, vec2};

use crate::editor::EditorOptions;
use crate::theme::Theme;

const MARGIN: f32 = 10.0;
const ROW: f32 = 22.0;
const TEXT_SIZE: f32 = 13.0;

const ALL: &str = "All";

/// The tags whose blocks show, `None` for every block.
pub(crate) fn shown<'a>(options: &'a EditorOptions, language: &'a Language) -> Option<&'a [String]> {
    let default = language.checked_tags()?;
    match options.palette_all {
        true => None,
        false => Some(options.palette_tags.as_deref().unwrap_or(default)),
    }
}

pub(crate) fn height(language: &Language) -> f32 {
    match language.checked_tags() {
        None => 0.0,
        Some(_) => (language.tags().len() + 1).div_ceil(2) as f32 * ROW + 2.0 * MARGIN,
    }
}

pub(crate) fn width(ctx: &egui::Context, language: &Language) -> f32 {
    if language.checked_tags().is_none() {
        return 0.0;
    }
    let widest = std::iter::once(ALL)
        .chain(language.tags().iter().map(String::as_str))
        .map(|tag| {
            ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(tag.to_owned(), FontId::proportional(TEXT_SIZE), Color32::WHITE)
                    .size()
                    .x
            })
        })
        .fold(0.0, f32::max);
    let spacing = ctx.global_style().spacing.clone();
    let checkbox = spacing.icon_width + spacing.icon_spacing;
    2.0 * (checkbox + widest) + 3.0 * MARGIN
}

/// Returns whether a box changed, which takes effect next frame.
pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    language: &Language,
    options: &mut EditorOptions,
    theme: &Theme,
) -> bool {
    let Some(default) = language.checked_tags() else {
        return false;
    };
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let mut panel = ui.new_child(UiBuilder::new().max_rect(rect));
    panel.set_clip_rect(rect);
    panel.painter().rect_filled(rect, 0.0, theme.palette);
    panel
        .painter()
        .hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, theme.divider));

    let tags = language.tags();
    let rows = (tags.len() + 1).div_ceil(2);
    let column = ((rect.width() - 3.0 * MARGIN) / 2.0).max(0.0);
    let mut checked = options.palette_tags.clone().unwrap_or_else(|| default.to_vec());
    let mut all = options.palette_all;
    let (mut changed, mut ticked) = (false, false);
    // Down the left column, then the right.
    for (index, tag) in std::iter::once(None).chain(tags.iter().map(Some)).enumerate() {
        let (x, y) = (index / rows, index % rows);
        let min = rect.min + vec2(MARGIN + x as f32 * (column + MARGIN), MARGIN + y as f32 * ROW);
        let cell = Rect::from_min_size(min, vec2(column, ROW));
        let mut ui = panel.new_child(
            UiBuilder::new()
                .max_rect(cell)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        ui.set_clip_rect(cell.intersect(rect));
        ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
        let label = |text: &str| RichText::new(text).size(TEXT_SIZE).color(theme.palette_heading);
        let Some(tag) = tag else {
            changed |= ui.checkbox(&mut all, label(ALL)).changed();
            continue;
        };
        let mut on = checked.contains(tag);
        if ui.checkbox(&mut on, label(tag)).changed() {
            ticked = true;
            match on {
                true => checked.push(tag.clone()),
                false => checked.retain(|other| other != tag),
            }
        }
    }
    if ticked {
        options.palette_tags = Some(checked);
    }
    options.palette_all = all;
    changed || ticked
}
