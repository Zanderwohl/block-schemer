//! The macOS menu bar.

use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, KeyboardShortcut};
use muda::accelerator::Accelerator;
use muda::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use super::Command;

pub struct NativeMenus {
    /// Dropping it would empty the menu bar.
    _menu: Menu,
    items: Vec<(Command, MenuItem)>,
    clicked: Receiver<Command>,
}

impl NativeMenus {
    /// Replaces the app's menu bar. Call once NSApp exists, and only once:
    /// muda keeps the first event handler for the life of the process.
    pub fn new(name: &str, ctx: &egui::Context) -> muda::Result<Self> {
        let items = Command::ALL.map(|command| {
            let accelerator = accelerator(command.shortcut());
            let label = match command {
                Command::Quit => format!("Quit {name}"),
                _ => command.label().to_owned(),
            };
            (command, MenuItem::with_id(command.id(), label, true, Some(accelerator)))
        });
        let item = |command| &items.iter().find(|(c, _)| *c == command).expect("every command has an item").1;
        let [new, open, save, save_as, quit, undo, redo] = [
            Command::New,
            Command::Open,
            Command::Save,
            Command::SaveAs,
            Command::Quit,
            Command::Undo,
            Command::Redo,
        ]
        .map(item);
        let separator = PredefinedMenuItem::separator();
        let about = AboutMetadata {
            name: Some(name.to_owned()),
            ..Default::default()
        };

        let app = Submenu::with_items(
            name,
            true,
            &[
                &PredefinedMenuItem::about(None, Some(about)),
                &separator,
                &PredefinedMenuItem::services(None),
                &separator,
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &separator,
                // Not the predefined Quit, whose `terminate:` skips the save prompt.
                quit,
            ],
        )?;
        let file = Submenu::with_items("File", true, &[new, open, &separator, save, save_as])?;
        let edit = Submenu::with_items("Edit", true, &[undo, redo])?;
        let window = Submenu::with_items(
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &separator,
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        )?;
        let menu = Menu::with_items(&[&app, &file, &edit, &window])?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();

        let (sender, clicked) = mpsc::channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(command) = Command::ALL.into_iter().find(|command| event.id() == command.id()) {
                let _ = sender.send(command);
                // An idle window would not otherwise draw the frame that handles it.
                ctx.request_repaint();
            }
        }));

        Ok(Self {
            _menu: menu,
            items: items.into(),
            clicked,
        })
    }

    pub fn set_enabled(&self, enabled: impl Fn(Command) -> bool) {
        for (command, item) in &self.items {
            let enabled = enabled(*command);
            if item.is_enabled() != enabled {
                item.set_enabled(enabled);
            }
        }
    }

    pub fn clicked(&self) -> Vec<Command> {
        self.clicked.try_iter().collect()
    }
}

fn accelerator(shortcut: KeyboardShortcut) -> Accelerator {
    let modifiers = shortcut.modifiers;
    let mut text = String::new();
    for (held, name) in [
        (modifiers.command, "cmd+"),
        (modifiers.shift, "shift+"),
        (modifiers.alt, "alt+"),
        (modifiers.ctrl, "ctrl+"),
    ] {
        if held {
            text.push_str(name);
        }
    }
    text.push_str(shortcut.logical_key.name());
    text.parse().expect("menu shortcuts name a key muda knows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_keep_their_modifiers() {
        use muda::accelerator::{Code, Modifiers};
        let cases = [
            (Command::Save, Modifiers::META, Code::KeyS),
            (Command::SaveAs, Modifiers::META | Modifiers::SHIFT, Code::KeyS),
            (Command::Undo, Modifiers::META, Code::KeyZ),
            (Command::Redo, Modifiers::META | Modifiers::SHIFT, Code::KeyZ),
            (Command::Quit, Modifiers::META, Code::KeyQ),
        ];
        for (command, modifiers, key) in cases {
            assert_eq!(accelerator(command.shortcut()), Accelerator::new(modifiers, key), "{command:?}");
        }
        for command in Command::ALL {
            accelerator(command.shortcut());
        }
    }
}
