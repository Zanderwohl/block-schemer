//! The side panel's tabs: the host's content, in the editor's order.

use std::collections::HashMap;
use std::sync::Arc;

use block_parse::host::{Tab, TabContent, TabId};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, CursorIcon, FontId, Galley, Key, Rect, Sense, TextEdit, UiBuilder, pos2, vec2};

use crate::editor::{EditorEvent, EditorOptions};
use crate::theme::Theme;

const STRIP_HEIGHT: f32 = 26.0;
const TAB_PAD: f32 = 10.0;
const CLOSE_SIZE: f32 = 16.0;
/// Tabs shrink to share the strip, down to this; past it they are clipped.
const MIN_TAB_WIDTH: f32 = 56.0;
const TITLE_SIZE: f32 = 13.0;
const INPUT_HEIGHT: f32 = 28.0;

#[derive(Debug, Default)]
pub(crate) struct TabState {
    /// Last frame's active tab, which new tabs open after.
    shown: Option<TabId>,
    /// What is typed on each console's line, not yet entered.
    typed: HashMap<TabId, String>,
}

/// Brings `order` and `active` up to date with the host's `tabs`. A closed
/// active tab hands over to its right-hand neighbor, else its left.
pub(crate) fn arrange(options: &mut EditorOptions, state: &mut TabState, tabs: &[Tab]) {
    let exists = |id: &TabId| tabs.iter().any(|tab| tab.id == *id);
    let order = &mut options.tab_order;
    let heir = options
        .active_tab
        .as_ref()
        .and_then(|active| order.iter().position(|id| id == active))
        .and_then(|at| order[at + 1..].iter().chain(order[..at].iter().rev()).find(|id| exists(id)))
        .cloned();
    let mut seen = Vec::with_capacity(order.len());
    order.retain(|id| exists(id) && !seen.contains(id) && {
        seen.push(id.clone());
        true
    });
    let mut at = state
        .shown
        .as_ref()
        .and_then(|shown| order.iter().position(|id| id == shown))
        .map_or(order.len(), |at| at + 1);
    for tab in tabs {
        if !order.contains(&tab.id) {
            order.insert(at, tab.id.clone());
            at += 1;
        }
    }
    if !options.active_tab.as_ref().is_some_and(exists) {
        options.active_tab = heir.or_else(|| order.first().cloned());
    }
    state.shown = options.active_tab.clone();
    state.typed.retain(|id, _| exists(id));
}

/// Call after [`arrange`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    options: &mut EditorOptions,
    state: &mut TabState,
    tabs: &[Tab],
    theme: &Theme,
    events: &mut Vec<EditorEvent>,
) {
    if rect.width() <= 0.0 {
        return;
    }
    let mut panel = ui.new_child(UiBuilder::new().max_rect(rect));
    panel.set_clip_rect(rect);
    panel.painter().rect_filled(rect, 0.0, theme.palette);
    let strip = Rect::from_min_size(rect.min, vec2(rect.width(), STRIP_HEIGHT));
    let body = Rect::from_min_max(pos2(rect.min.x, strip.max.y), rect.max);

    let ordered: Vec<&Tab> = options
        .tab_order
        .iter()
        .filter_map(|id| tabs.iter().find(|tab| tab.id == *id))
        .collect();
    if ordered.is_empty() {
        panel.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            "Nothing to show",
            FontId::proportional(TITLE_SIZE),
            theme.palette_heading,
        );
        return;
    }
    strip_ui(&mut panel, strip, id, options, &ordered, theme, events);

    let Some(tab) = ordered.iter().find(|tab| Some(&tab.id) == options.active_tab.as_ref()) else {
        return;
    };
    match &tab.content {
        TabContent::Text(text) => {
            let mut ui = panel.new_child(UiBuilder::new().max_rect(body));
            egui::ScrollArea::vertical()
                .id_salt(id.with(("tab_scroll", &tab.id)))
                .auto_shrink(false)
                .show(&mut ui, |ui| read_only(ui, text, id.with(("tab_text", &tab.id))));
        }
        TabContent::Console { output } => {
            let input = Rect::from_min_max(pos2(body.min.x, body.max.y - INPUT_HEIGHT), body.max);
            let output_rect = Rect::from_min_max(body.min, pos2(body.max.x, input.min.y));
            let mut ui = panel.new_child(UiBuilder::new().max_rect(output_rect));
            egui::ScrollArea::vertical()
                .id_salt(id.with(("tab_scroll", &tab.id)))
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show(&mut ui, |ui| read_only(ui, output, id.with(("tab_text", &tab.id))));
            panel.painter().hline(body.x_range(), input.min.y, (1.0, theme.divider));
            let mut ui = panel.new_child(UiBuilder::new().max_rect(input.shrink2(vec2(4.0, 3.0))));
            ui.horizontal_centered(|ui| {
                ui.monospace("›");
                let line = state.typed.entry(tab.id.clone()).or_default();
                let field = TextEdit::singleline(line)
                    .id(id.with(("console_line", &tab.id)))
                    .font(egui::TextStyle::Monospace)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY);
                let response = ui.add(field);
                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    events.push(EditorEvent::ConsoleInput {
                        tab: tab.id.clone(),
                        line: std::mem::take(line),
                    });
                    response.request_focus();
                }
            });
        }
    }
}

/// Through `&str`: read-only, but it can be selected and copied.
fn read_only(ui: &mut egui::Ui, mut text: &str, id: egui::Id) {
    let field = TextEdit::multiline(&mut text)
        .id(id)
        .font(egui::TextStyle::Monospace)
        .desired_width(f32::INFINITY)
        .min_size(ui.available_size());
    ui.add(field);
}

/// Picks, closes and reorders tabs.
fn strip_ui(
    panel: &mut egui::Ui,
    strip: Rect,
    id: egui::Id,
    options: &mut EditorOptions,
    ordered: &[&Tab],
    theme: &Theme,
    events: &mut Vec<EditorEvent>,
) {
    let painter = panel.painter().clone();
    painter.rect_filled(strip, 0.0, theme.canvas);
    let titles: Vec<Arc<Galley>> = ordered
        .iter()
        .map(|tab| title(&painter, &tab.title, f32::INFINITY, theme.palette_heading))
        .collect();
    let natural: Vec<f32> = ordered
        .iter()
        .zip(&titles)
        .map(|(tab, galley)| 2.0 * TAB_PAD + galley.size().x + if tab.closable { CLOSE_SIZE } else { 0.0 })
        .collect();
    let share = (strip.width() / ordered.len() as f32).max(MIN_TAB_WIDTH);
    let squeeze = natural.iter().sum::<f32>() > strip.width();
    let widths: Vec<f32> = natural.iter().map(|&w| if squeeze { w.min(share) } else { w }).collect();
    let mut x = strip.min.x;
    let rects: Vec<Rect> = widths
        .iter()
        .map(|&w| {
            let rect = Rect::from_min_size(pos2(x, strip.min.y), vec2(w, STRIP_HEIGHT));
            x += w;
            rect
        })
        .collect();
    painter.hline(strip.x_range(), strip.max.y - 0.5, (1.0, theme.divider));

    let mut moved = None;
    for (index, ((tab, rect), width)) in ordered.iter().zip(&rects).zip(&widths).enumerate() {
        let active = options.active_tab.as_ref() == Some(&tab.id);
        let response = panel
            .interact(*rect, id.with(("tab", &tab.id)), Sense::click_and_drag())
            .on_hover_text(&tab.title);
        if response.clicked() || response.drag_started() {
            options.active_tab = Some(tab.id.clone());
        }
        if response.dragged()
            && let Some(at) = response.interact_pointer_pos()
        {
            panel.ctx().set_cursor_icon(CursorIcon::Grabbing);
            // Against the others packed without it, so where it lands depends
            // on the pointer alone and never flips back and forth.
            let mut left = strip.min.x;
            let mut to = 0;
            for (_, w) in widths.iter().enumerate().filter(|&(other, _)| other != index) {
                if at.x > left + w / 2.0 {
                    to += 1;
                }
                left += w;
            }
            if to != index {
                moved = Some((index, to));
            }
        }
        let close = tab.closable.then(|| {
            let rect = Rect::from_center_size(
                pos2(rect.max.x - TAB_PAD / 2.0 - CLOSE_SIZE / 2.0, rect.center().y),
                vec2(CLOSE_SIZE, CLOSE_SIZE),
            );
            (rect, panel.interact(rect, id.with(("tab_close", &tab.id)), Sense::click()))
        });
        if close.as_ref().is_some_and(|(_, close)| close.clicked()) || response.middle_clicked() {
            events.push(EditorEvent::CloseTab(tab.id.clone()));
        }

        if active {
            painter.rect_filled(*rect, 0.0, theme.palette);
        } else if response.hovered() {
            painter.rect_filled(*rect, 0.0, theme.divider.gamma_multiply(0.5));
        }
        painter.vline(rect.max.x - 0.5, rect.y_range(), (1.0, theme.divider));
        let ink = if active {
            panel.visuals().strong_text_color()
        } else {
            theme.palette_heading
        };
        let room = width - 2.0 * TAB_PAD - if tab.closable { CLOSE_SIZE } else { 0.0 };
        let galley = title(&painter, &tab.title, room.max(0.0), ink);
        let at = pos2(rect.min.x + TAB_PAD, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, ink);
        if let Some((rect, close)) = close {
            if close.hovered() {
                painter.rect_filled(rect, 3.0, theme.divider);
            }
            painter.text(rect.center(), Align2::CENTER_CENTER, "×", FontId::proportional(TITLE_SIZE), ink);
        }
    }
    // `arrange` left the order matching what was drawn.
    if let Some((from, to)) = moved {
        let tab = options.tab_order.remove(from);
        options.tab_order.insert(to, tab);
    }
}

fn title(painter: &egui::Painter, text: &str, width: f32, ink: egui::Color32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(TITLE_SIZE), ink);
    job.wrap = TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(ids: &[&str]) -> Vec<Tab> {
        ids.iter()
            .map(|&id| Tab {
                id: TabId::from(id),
                title: id.into(),
                closable: true,
                content: TabContent::Text(String::new()),
            })
            .collect()
    }

    fn ids(ids: &[&str]) -> Vec<TabId> {
        ids.iter().map(|&id| TabId::from(id)).collect()
    }

    #[test]
    fn new_tabs_open_after_the_active_one_in_the_hosts_order() {
        let mut options = EditorOptions::default();
        let mut state = TabState::default();
        arrange(&mut options, &mut state, &tabs(&["a", "b", "c"]));
        assert_eq!(options.tab_order, ids(&["a", "b", "c"]));
        assert_eq!(options.active_tab, Some(TabId::from("a")));

        // The host brings its new tab to the front in the same frame.
        options.active_tab = Some(TabId::from("y"));
        arrange(&mut options, &mut state, &tabs(&["c", "x", "b", "a", "y"]));
        assert_eq!(options.tab_order, ids(&["a", "x", "y", "b", "c"]));
        assert_eq!(options.active_tab, Some(TabId::from("y")));
    }

    #[test]
    fn a_closed_active_tab_hands_over_to_a_neighbor() {
        let mut options = EditorOptions {
            tab_order: ids(&["a", "b", "c", "a"]),
            active_tab: Some(TabId::from("b")),
            ..EditorOptions::default()
        };
        let mut state = TabState::default();
        arrange(&mut options, &mut state, &tabs(&["a", "c"]));
        assert_eq!(options.tab_order, ids(&["a", "c"]), "gone and repeated ids are dropped");
        assert_eq!(options.active_tab, Some(TabId::from("c")), "the right-hand one");

        arrange(&mut options, &mut state, &tabs(&["a"]));
        assert_eq!(options.active_tab, Some(TabId::from("a")), "else the left-hand one");

        arrange(&mut options, &mut state, &[]);
        assert_eq!((options.tab_order.len(), options.active_tab), (0, None));
    }
}
