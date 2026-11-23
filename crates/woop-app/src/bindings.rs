//! The keyboard shortcuts in effect: each platform's defaults, changed by
//! the `[keys]` table of the configuration (ADR 0021).

use iced::keyboard::key::Named;
use iced::keyboard::{Key as WindowKey, Modifiers as WindowModifiers};
use woop_config::{Action, ChordKey, KeyChord, NamedKey};

use crate::keys::Shortcuts;

/// Shortcuts and the actions they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    entries: Vec<(KeyChord, Action)>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self::new(Shortcuts::platform(), &[])
    }
}

impl Bindings {
    /// The defaults of `shortcuts`' platform with the user's `changes`: an
    /// action given a shortcut loses its default ones, and the shortcut is
    /// taken from any action that had it; `None` removes an action's
    /// shortcuts.
    pub fn new(shortcuts: Shortcuts, changes: &[(Action, Option<KeyChord>)]) -> Self {
        let mut entries = defaults(shortcuts);
        for &(action, chord) in changes {
            entries.retain(|&(_, bound)| bound != action);
            // On macOS, Option with a character types another character (å,
            // @ on some layouts): such a shortcut would stop it being typed.
            let types_on_macos = |chord: &KeyChord| {
                chord.alt && !chord.ctrl && !chord.logo && matches!(chord.key, ChordKey::Char(_))
            };
            if let Some(chord) = chord
                && !(shortcuts == Shortcuts::MacOs && types_on_macos(&chord))
            {
                entries.retain(|&(bound, _)| bound != chord);
                entries.push((chord, action));
            }
        }
        Self { entries }
    }

    /// The action bound to a key press, if any.
    pub fn action(&self, key: &WindowKey, modifiers: WindowModifiers) -> Option<Action> {
        self.entries
            .iter()
            .find(|(chord, _)| chord_matches(chord, key, modifiers))
            .map(|&(_, action)| action)
    }

    /// The first shortcut of `action`, if it has one.
    pub fn chord(&self, action: Action) -> Option<KeyChord> {
        self.entries
            .iter()
            .find(|&&(_, bound)| bound == action)
            .map(|&(chord, _)| chord)
    }
}

/// Each platform's default shortcuts. None of these keys is used by the
/// shells: Ctrl+Shift with a letter (Cmd on macOS), Ctrl+Tab, Alt with an
/// arrow (Cmd+Option on macOS, where Option+arrow moves by words), and
/// Ctrl+Shift+Up/Down (Cmd+Up/Down; Ctrl+Up/Down alone are used by line
/// editors).
fn defaults(shortcuts: Shortcuts) -> Vec<(KeyChord, Action)> {
    let letter = ChordKey::Char;
    let named = ChordKey::Named;
    let tabs = [
        (KeyChord::ctrl(named(NamedKey::Tab)), Action::NextTab),
        (
            KeyChord::ctrl(named(NamedKey::Tab)).shift(),
            Action::PreviousTab,
        ),
    ];
    let platform: Vec<(KeyChord, Action)> = match shortcuts {
        Shortcuts::MacOs => {
            let cmd = KeyChord::logo;
            vec![
                (cmd(letter('t')), Action::NewTab),
                (cmd(letter('w')), Action::ClosePane),
                (cmd(letter('d')), Action::SplitRight),
                (cmd(letter('d')).shift(), Action::SplitDown),
                (cmd(letter('p')).shift(), Action::CommandPalette),
                (cmd(named(NamedKey::Left)).with_alt(), Action::FocusLeft),
                (cmd(named(NamedKey::Right)).with_alt(), Action::FocusRight),
                (cmd(named(NamedKey::Up)).with_alt(), Action::FocusUp),
                (cmd(named(NamedKey::Down)).with_alt(), Action::FocusDown),
                (cmd(named(NamedKey::Up)), Action::PreviousBlock),
                (cmd(named(NamedKey::Down)), Action::NextBlock),
            ]
        }
        Shortcuts::Windows | Shortcuts::Unix => {
            let ctrl_shift = |key| KeyChord::ctrl(key).shift();
            vec![
                (ctrl_shift(letter('t')), Action::NewTab),
                (ctrl_shift(letter('w')), Action::ClosePane),
                (ctrl_shift(letter('d')), Action::SplitRight),
                (ctrl_shift(letter('e')), Action::SplitDown),
                (ctrl_shift(letter('p')), Action::CommandPalette),
                (KeyChord::alt(named(NamedKey::Left)), Action::FocusLeft),
                (KeyChord::alt(named(NamedKey::Right)), Action::FocusRight),
                (KeyChord::alt(named(NamedKey::Up)), Action::FocusUp),
                (KeyChord::alt(named(NamedKey::Down)), Action::FocusDown),
                (ctrl_shift(named(NamedKey::Up)), Action::PreviousBlock),
                (ctrl_shift(named(NamedKey::Down)), Action::NextBlock),
            ]
        }
    };
    tabs.into_iter().chain(platform).collect()
}

/// Whether a key press is the shortcut: the same key and exactly the same
/// modifiers.
fn chord_matches(chord: &KeyChord, key: &WindowKey, modifiers: WindowModifiers) -> bool {
    let same_modifiers = chord.ctrl == modifiers.control()
        && chord.alt == modifiers.alt()
        && chord.shift == modifiers.shift()
        && chord.logo == modifiers.logo();
    same_modifiers
        && match (chord.key, key) {
            (ChordKey::Char(expected), WindowKey::Character(chars)) => {
                chars.chars().next().and_then(|ch| ch.to_lowercase().next()) == Some(expected)
            }
            (ChordKey::Char(' '), WindowKey::Named(Named::Space)) => true,
            (ChordKey::Named(expected), WindowKey::Named(named)) => {
                named_key(*named) == Some(expected)
            }
            _ => false,
        }
}

fn named_key(named: Named) -> Option<NamedKey> {
    Some(match named {
        Named::ArrowUp => NamedKey::Up,
        Named::ArrowDown => NamedKey::Down,
        Named::ArrowLeft => NamedKey::Left,
        Named::ArrowRight => NamedKey::Right,
        Named::Tab => NamedKey::Tab,
        Named::Enter => NamedKey::Enter,
        Named::Space => NamedKey::Space,
        Named::Escape => NamedKey::Escape,
        Named::Backspace => NamedKey::Backspace,
        Named::Delete => NamedKey::Delete,
        Named::Insert => NamedKey::Insert,
        Named::Home => NamedKey::Home,
        Named::End => NamedKey::End,
        Named::PageUp => NamedKey::PageUp,
        Named::PageDown => NamedKey::PageDown,
        Named::F1 => NamedKey::F(1),
        Named::F2 => NamedKey::F(2),
        Named::F3 => NamedKey::F(3),
        Named::F4 => NamedKey::F(4),
        Named::F5 => NamedKey::F(5),
        Named::F6 => NamedKey::F(6),
        Named::F7 => NamedKey::F(7),
        Named::F8 => NamedKey::F(8),
        Named::F9 => NamedKey::F(9),
        Named::F10 => NamedKey::F(10),
        Named::F11 => NamedKey::F(11),
        Named::F12 => NamedKey::F(12),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(ch: &str) -> WindowKey {
        WindowKey::Character(ch.into())
    }

    #[test]
    fn defaults_follow_the_platform() {
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        let windows = Bindings::new(Shortcuts::Windows, &[]);
        assert_eq!(
            windows.action(&character("T"), ctrl_shift),
            Some(Action::NewTab)
        );
        assert_eq!(
            windows.action(&WindowKey::Named(Named::ArrowLeft), WindowModifiers::ALT),
            Some(Action::FocusLeft)
        );
        let mac = Bindings::new(Shortcuts::MacOs, &[]);
        assert_eq!(
            mac.action(&character("t"), WindowModifiers::LOGO),
            Some(Action::NewTab)
        );
        // Option+arrow moves by words on macOS.
        assert_eq!(
            mac.action(&WindowKey::Named(Named::ArrowLeft), WindowModifiers::ALT),
            None
        );
        // Modifiers must match exactly.
        assert_eq!(windows.action(&character("t"), WindowModifiers::CTRL), None);
    }

    #[test]
    fn option_with_a_character_stays_for_typing_on_macos() {
        let option_e = KeyChord::alt(ChordKey::Char('e'));
        let changes = [(Action::NewTab, Some(option_e))];
        let mac = Bindings::new(Shortcuts::MacOs, &changes);
        assert_eq!(mac.action(&character("e"), WindowModifiers::ALT), None);
        // Elsewhere Alt+E is not a typing key.
        let linux = Bindings::new(Shortcuts::Unix, &changes);
        assert_eq!(
            linux.action(&character("e"), WindowModifiers::ALT),
            Some(Action::NewTab)
        );
    }

    #[test]
    fn changes_replace_remove_and_move_shortcuts() {
        let ctrl_alt = |key| KeyChord::ctrl(ChordKey::Named(key)).with_alt();
        let bindings = Bindings::new(
            Shortcuts::Unix,
            &[
                (Action::FocusLeft, Some(ctrl_alt(NamedKey::Left))),
                (Action::SplitDown, None),
                // Takes Ctrl+Shift+T from New tab.
                (
                    Action::CopyLastCommandLine,
                    Some(KeyChord::ctrl(ChordKey::Char('t')).shift()),
                ),
            ],
        );
        let left = WindowKey::Named(Named::ArrowLeft);
        assert_eq!(bindings.action(&left, WindowModifiers::ALT), None);
        assert_eq!(
            bindings.action(&left, WindowModifiers::CTRL | WindowModifiers::ALT),
            Some(Action::FocusLeft)
        );
        assert_eq!(bindings.chord(Action::SplitDown), None);
        assert_eq!(
            bindings.action(
                &character("T"),
                WindowModifiers::CTRL | WindowModifiers::SHIFT
            ),
            Some(Action::CopyLastCommandLine)
        );
        assert_eq!(bindings.chord(Action::NewTab), None);
    }
}
