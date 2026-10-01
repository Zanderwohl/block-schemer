//! Separate from the editor so the host can place it.

use block_parse::host::{RunCommand, RunStatus};

/// Emits commands; runs nothing.
pub struct RunToolbar<'a> {
    pub status: RunStatus,
    /// False when there are errors and the editor is set not to start with them.
    pub can_start: bool,
    pub supports: &'a dyn Fn(RunCommand) -> bool,
}

impl RunToolbar<'_> {
    /// The command clicked, if any.
    pub fn show(&self, ui: &mut egui::Ui) -> Option<RunCommand> {
        let idle = self.status == RunStatus::Idle;
        // Glyphs egui's default fonts have; ▶ (U+25B6) is not among them.
        let mut buttons = [
            (RunCommand::Start, "⏵", "Play", self.can_start && idle),
            (RunCommand::Stop, "⏹", "Stop", !idle),
        ];
        // So they read in this order when placed from the right.
        if ui.layout().prefer_right_to_left() {
            buttons.reverse();
        }
        let mut clicked = None;
        ui.horizontal(|ui| {
            for (command, glyph, tip, enabled) in buttons {
                let button = egui::Button::new(egui::RichText::new(glyph).size(16.0)).min_size(egui::vec2(28.0, 0.0));
                let enabled = enabled && (self.supports)(command);
                if ui.add_enabled(enabled, button).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                    clicked = Some(command);
                }
            }
        });
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, pos2};

    /// What clicking at `at` sends, from a toolbar at the screen's top left.
    fn click(status: RunStatus, stoppable: bool, at: Pos2) -> Option<RunCommand> {
        let ctx = egui::Context::default();
        let toolbar = RunToolbar {
            status,
            can_start: true,
            supports: &|command| stoppable || command != RunCommand::Stop,
        };
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut clicked = None;
        for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| clicked = clicked.or(toolbar.show(ui))).textures_delta.clear();
        }
        clicked
    }

    #[test]
    fn play_starts_when_idle_and_stop_only_while_running() {
        let (play, stop) = (pos2(14.0, 9.0), pos2(50.0, 9.0));
        assert_eq!(click(RunStatus::Idle, true, play), Some(RunCommand::Start));
        assert_eq!(click(RunStatus::Idle, true, stop), None);
        assert_eq!(click(RunStatus::Running, true, play), None);
        assert_eq!(click(RunStatus::Running, true, stop), Some(RunCommand::Stop));
        assert_eq!(click(RunStatus::Running, false, stop), None, "the runner cannot stop");
    }
}
