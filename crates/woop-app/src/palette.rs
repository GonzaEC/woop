//! The command palette: search Woop's actions by name and run one.
//!
//! While the palette is open the window reads the keyboard itself: typed
//! text filters the commands, Up and Down choose one, Enter runs it and
//! Escape closes the palette. No key reaches a pane meanwhile.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::bindings::Bindings;
use crate::keys::Shortcuts;

/// Longest search text kept.
const MAX_QUERY_CHARS: usize = 100;

/// An action the palette can run: the actions that can also be given a
/// shortcut in the configuration (ADR 0021).
pub use woop_config::Action as Command;

/// The commands the palette lists, in order: every action except opening
/// the palette itself.
fn listed() -> impl Iterator<Item = Command> {
    Command::ALL
        .into_iter()
        .filter(|&command| command != Command::CommandPalette)
}

/// The name shown and searched.
pub fn label(command: Command) -> &'static str {
    match command {
        Command::NewTab => "New tab",
        Command::ClosePane => "Close pane",
        Command::NextTab => "Next tab",
        Command::PreviousTab => "Previous tab",
        Command::SplitRight => "Split pane right",
        Command::SplitDown => "Split pane down",
        Command::FocusLeft => "Focus pane on the left",
        Command::FocusRight => "Focus pane on the right",
        Command::FocusUp => "Focus pane above",
        Command::FocusDown => "Focus pane below",
        Command::CommandPalette => "Command palette",
        Command::CopyLastCommandLine => "Copy last command",
        Command::CopyLastOutput => "Copy last command's output",
        Command::ToggleLastOutput => "Collapse or expand last command's output",
        Command::PreviousBlock => "Go to previous command",
        Command::NextBlock => "Go to next command",
        Command::CopySelection => "Copy selection",
        Command::Paste => "Paste",
        Command::ScrollToBottom => "Scroll to the latest output",
        Command::CopyConfigPath => "Copy configuration file path",
        Command::ReloadConfig => "Reload configuration",
    }
}

/// The shortcut shown for a command: the one in effect, from the bindings,
/// or for copying and pasting the platform's own keys.
pub fn shortcut(command: Command, shortcuts: Shortcuts, bindings: &Bindings) -> Option<String> {
    let fixed = |windows: &str, unix: &str, macos: &str| {
        Some(
            match shortcuts {
                Shortcuts::Windows => windows,
                Shortcuts::Unix => unix,
                Shortcuts::MacOs => macos,
            }
            .to_owned(),
        )
    };
    match command {
        Command::CopySelection => fixed("Ctrl+C", "Ctrl+Shift+C", "Cmd+C"),
        Command::Paste => fixed("Ctrl+V", "Ctrl+Shift+V", "Cmd+V"),
        _ => bindings
            .chord(command)
            .map(|chord| chord.label(shortcuts == Shortcuts::MacOs)),
    }
}

/// Whether every word of `query` appears in the label, ignoring case.
fn matches(command: Command, query: &str) -> bool {
    let label = label(command).to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| label.contains(word))
}

/// What a key press in the palette leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The palette stays open.
    Stay,
    /// Close the palette.
    Close,
    /// Close the palette and run the command.
    Run(Command),
}

/// The open palette: what was typed and which command is chosen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Palette {
    query: String,
    selected: usize,
}

impl Palette {
    /// The text typed so far.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The index of the chosen command among [`Palette::matches`].
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The commands matching the text typed, in order.
    pub fn matches(&self) -> Vec<Command> {
        listed()
            .filter(|&command| matches(command, &self.query))
            .collect()
    }

    /// Handles a key press. `text` is what the key typed, if anything.
    pub fn key(&mut self, key: &Key, modifiers: Modifiers, text: Option<&str>) -> Outcome {
        let count = self.matches().len();
        match key {
            Key::Named(Named::Escape) => return Outcome::Close,
            Key::Named(Named::Enter) => {
                return self
                    .matches()
                    .get(self.selected)
                    .map_or(Outcome::Stay, |&command| Outcome::Run(command));
            }
            Key::Named(Named::ArrowDown) if count > 0 => {
                self.selected = (self.selected + 1) % count;
                return Outcome::Stay;
            }
            Key::Named(Named::ArrowUp) if count > 0 => {
                self.selected = (self.selected + count - 1) % count;
                return Outcome::Stay;
            }
            Key::Named(Named::Backspace) => {
                self.query.pop();
                self.selected = 0;
                return Outcome::Stay;
            }
            _ => {}
        }
        // Shortcuts (Ctrl or Cmd with a key) are not text; AltGr, reported
        // as Ctrl+Alt, types characters.
        let shortcut = (modifiers.control() && !modifiers.alt()) || modifiers.logo();
        if !shortcut && let Some(text) = text {
            let typed: String = text.chars().filter(|ch| !ch.is_control()).collect();
            if !typed.is_empty()
                && self.query.chars().count() + typed.chars().count() <= MAX_QUERY_CHARS
            {
                self.query.push_str(&typed);
                self.selected = 0;
            }
        }
        Outcome::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(palette: &mut Palette, key: Key, text: Option<&str>) -> Outcome {
        palette.key(&key, Modifiers::empty(), text)
    }

    fn type_text(palette: &mut Palette, text: &str) {
        for ch in text.chars() {
            let typed = ch.to_string();
            press(palette, Key::Character(typed.as_str().into()), Some(&typed));
        }
    }

    #[test]
    fn lists_every_command_until_something_is_typed() {
        assert_eq!(Palette::default().matches(), listed().collect::<Vec<_>>());
    }

    #[test]
    fn filters_by_every_word_ignoring_case() {
        let mut palette = Palette::default();
        type_text(&mut palette, "COPY out");
        assert_eq!(palette.matches(), [Command::CopyLastOutput]);

        let mut palette = Palette::default();
        type_text(&mut palette, "split");
        assert_eq!(palette.matches(), [Command::SplitRight, Command::SplitDown]);

        let mut palette = Palette::default();
        type_text(&mut palette, "nothing like this");
        assert!(palette.matches().is_empty());
        assert_eq!(
            press(&mut palette, Key::Named(Named::Enter), None),
            Outcome::Stay
        );
    }

    #[test]
    fn arrows_choose_and_enter_runs() {
        let mut palette = Palette::default();
        type_text(&mut palette, "split");
        assert_eq!(
            press(&mut palette, Key::Named(Named::Enter), None),
            Outcome::Run(Command::SplitRight)
        );
        press(&mut palette, Key::Named(Named::ArrowDown), None);
        assert_eq!(
            press(&mut palette, Key::Named(Named::Enter), None),
            Outcome::Run(Command::SplitDown)
        );
        // Wraps around.
        press(&mut palette, Key::Named(Named::ArrowDown), None);
        assert_eq!(palette.selected(), 0);
        press(&mut palette, Key::Named(Named::ArrowUp), None);
        assert_eq!(palette.selected(), 1);
    }

    #[test]
    fn typing_or_deleting_starts_the_choice_again() {
        let mut palette = Palette::default();
        press(&mut palette, Key::Named(Named::ArrowDown), None);
        type_text(&mut palette, "c");
        assert_eq!(palette.selected(), 0);
        press(&mut palette, Key::Named(Named::Backspace), None);
        assert_eq!(palette.query(), "");
    }

    #[test]
    fn escape_closes_and_shortcuts_are_not_typed() {
        let mut palette = Palette::default();
        assert_eq!(
            press(&mut palette, Key::Named(Named::Escape), None),
            Outcome::Close
        );
        palette.key(&Key::Character("v".into()), Modifiers::CTRL, Some("\u{16}"));
        palette.key(&Key::Character("v".into()), Modifiers::LOGO, Some("v"));
        assert_eq!(palette.query(), "");
        // AltGr types.
        palette.key(
            &Key::Character("q".into()),
            Modifiers::CTRL | Modifiers::ALT,
            Some("@"),
        );
        assert_eq!(palette.query(), "@");
    }

    #[test]
    fn the_query_is_bounded() {
        let mut palette = Palette::default();
        type_text(&mut palette, &"x".repeat(MAX_QUERY_CHARS + 20));
        assert_eq!(palette.query().chars().count(), MAX_QUERY_CHARS);
    }

    #[test]
    fn shows_the_shortcuts_in_effect() {
        let shown =
            |command, shortcuts, bindings: &Bindings| shortcut(command, shortcuts, bindings);
        let windows = Bindings::new(Shortcuts::Windows, &[]);
        let mac = Bindings::new(Shortcuts::MacOs, &[]);
        assert_eq!(
            shown(Command::NewTab, Shortcuts::Windows, &windows).as_deref(),
            Some("Ctrl+Shift+T")
        );
        assert_eq!(
            shown(Command::NewTab, Shortcuts::MacOs, &mac).as_deref(),
            Some("Cmd+T")
        );
        assert_eq!(
            shown(Command::FocusLeft, Shortcuts::MacOs, &mac).as_deref(),
            Some("Cmd+Option+Left")
        );
        assert_eq!(
            shown(Command::Paste, Shortcuts::Unix, &windows).as_deref(),
            Some("Ctrl+Shift+V")
        );
        assert_eq!(
            shown(Command::ReloadConfig, Shortcuts::Windows, &windows),
            None
        );
        // A shortcut from the configuration.
        let configured = Bindings::new(
            Shortcuts::Windows,
            &[(
                Command::ReloadConfig,
                Some(woop_config::KeyChord::parse("Ctrl+Shift+R").expect("valid")),
            )],
        );
        assert_eq!(
            shown(Command::ReloadConfig, Shortcuts::Windows, &configured).as_deref(),
            Some("Ctrl+Shift+R")
        );
    }

    #[test]
    fn the_palette_does_not_list_itself() {
        assert!(
            !Palette::default()
                .matches()
                .contains(&Command::CommandPalette)
        );
    }
}
