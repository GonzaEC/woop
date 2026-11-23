//! The command palette: search Woop's actions by name and run one.
//!
//! While the palette is open the window reads the keyboard itself: typed
//! text filters the commands, Up and Down choose one, Enter runs it and
//! Escape closes the palette. No key reaches a pane meanwhile.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::keys::Shortcuts;

/// Longest search text kept.
const MAX_QUERY_CHARS: usize = 100;

/// An action the palette can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Open a new tab.
    NewTab,
    /// Close the focused pane.
    ClosePane,
    /// Show the next tab.
    NextTab,
    /// Show the previous tab.
    PreviousTab,
    /// Split the focused pane, with the new pane on the right.
    SplitRight,
    /// Split the focused pane, with the new pane below.
    SplitDown,
    /// Focus the pane on the left.
    FocusLeft,
    /// Focus the pane on the right.
    FocusRight,
    /// Focus the pane above.
    FocusUp,
    /// Focus the pane below.
    FocusDown,
    /// Copy the last finished command.
    CopyLastCommandLine,
    /// Copy the output of the last finished command.
    CopyLastOutput,
    /// Collapse or expand the output of the last finished command.
    ToggleLastOutput,
    /// Move the view to the previous command block.
    PreviousBlock,
    /// Move the view to the next command block.
    NextBlock,
    /// Copy the selected text.
    CopySelection,
    /// Paste the clipboard.
    Paste,
    /// Move the view back to the latest output.
    ScrollToBottom,
    /// Copy the path of the configuration file.
    CopyConfigPath,
    /// Read the configuration file again now.
    ReloadConfig,
}

impl Command {
    /// Every command, in the order the palette lists them.
    pub const ALL: [Self; 20] = [
        Self::NewTab,
        Self::ClosePane,
        Self::NextTab,
        Self::PreviousTab,
        Self::SplitRight,
        Self::SplitDown,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::CopyLastCommandLine,
        Self::CopyLastOutput,
        Self::ToggleLastOutput,
        Self::PreviousBlock,
        Self::NextBlock,
        Self::CopySelection,
        Self::Paste,
        Self::ScrollToBottom,
        Self::CopyConfigPath,
        Self::ReloadConfig,
    ];

    /// The name shown and searched.
    pub fn label(self) -> &'static str {
        match self {
            Self::NewTab => "New tab",
            Self::ClosePane => "Close pane",
            Self::NextTab => "Next tab",
            Self::PreviousTab => "Previous tab",
            Self::SplitRight => "Split pane right",
            Self::SplitDown => "Split pane down",
            Self::FocusLeft => "Focus pane on the left",
            Self::FocusRight => "Focus pane on the right",
            Self::FocusUp => "Focus pane above",
            Self::FocusDown => "Focus pane below",
            Self::CopyLastCommandLine => "Copy last command",
            Self::CopyLastOutput => "Copy last command's output",
            Self::ToggleLastOutput => "Collapse or expand last command's output",
            Self::PreviousBlock => "Go to previous command",
            Self::NextBlock => "Go to next command",
            Self::CopySelection => "Copy selection",
            Self::Paste => "Paste",
            Self::ScrollToBottom => "Scroll to the latest output",
            Self::CopyConfigPath => "Copy configuration file path",
            Self::ReloadConfig => "Reload configuration",
        }
    }

    /// The keyboard shortcut for the command, if it has one.
    pub fn shortcut(self, shortcuts: Shortcuts) -> Option<&'static str> {
        let mac = shortcuts == Shortcuts::MacOs;
        let pick = |other: &'static str, macos: &'static str| Some(if mac { macos } else { other });
        match self {
            Self::NewTab => pick("Ctrl+Shift+T", "Cmd+T"),
            Self::ClosePane => pick("Ctrl+Shift+W", "Cmd+W"),
            Self::NextTab => Some("Ctrl+Tab"),
            Self::PreviousTab => Some("Ctrl+Shift+Tab"),
            Self::SplitRight => pick("Ctrl+Shift+D", "Cmd+D"),
            Self::SplitDown => pick("Ctrl+Shift+E", "Cmd+Shift+D"),
            Self::FocusLeft => pick("Alt+Left", "Cmd+Option+Left"),
            Self::FocusRight => pick("Alt+Right", "Cmd+Option+Right"),
            Self::FocusUp => pick("Alt+Up", "Cmd+Option+Up"),
            Self::FocusDown => pick("Alt+Down", "Cmd+Option+Down"),
            Self::PreviousBlock => pick("Ctrl+Shift+Up", "Cmd+Up"),
            Self::NextBlock => pick("Ctrl+Shift+Down", "Cmd+Down"),
            Self::CopySelection => match shortcuts {
                Shortcuts::Windows => Some("Ctrl+C"),
                Shortcuts::Unix => Some("Ctrl+Shift+C"),
                Shortcuts::MacOs => Some("Cmd+C"),
            },
            Self::Paste => match shortcuts {
                Shortcuts::Windows => Some("Ctrl+V"),
                Shortcuts::Unix => Some("Ctrl+Shift+V"),
                Shortcuts::MacOs => Some("Cmd+V"),
            },
            Self::CopyLastCommandLine
            | Self::CopyLastOutput
            | Self::ToggleLastOutput
            | Self::ScrollToBottom
            | Self::CopyConfigPath
            | Self::ReloadConfig => None,
        }
    }

    /// Whether every word of `query` appears in the label, ignoring case.
    fn matches(self, query: &str) -> bool {
        let label = self.label().to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|word| label.contains(word))
    }
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
        Command::ALL
            .into_iter()
            .filter(|command| command.matches(&self.query))
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
        assert_eq!(Palette::default().matches(), Command::ALL.to_vec());
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
    fn shortcuts_follow_the_platform() {
        assert_eq!(
            Command::NewTab.shortcut(Shortcuts::Windows),
            Some("Ctrl+Shift+T")
        );
        assert_eq!(Command::NewTab.shortcut(Shortcuts::MacOs), Some("Cmd+T"));
        assert_eq!(
            Command::Paste.shortcut(Shortcuts::Unix),
            Some("Ctrl+Shift+V")
        );
        assert_eq!(Command::ReloadConfig.shortcut(Shortcuts::Windows), None);
    }
}
