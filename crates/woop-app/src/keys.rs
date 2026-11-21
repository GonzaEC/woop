//! Translation of window keyboard events into terminal input and shortcuts.

use iced::keyboard::key::Named;
use iced::keyboard::{Key as WindowKey, Modifiers as WindowModifiers};
use woop_term::TerminalMode;
use woop_term::input::{self, Key, Modifiers};

/// What a key press means for the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    /// Bytes to send to the shell.
    Input(Vec<u8>),
    /// Paste the clipboard contents.
    Paste,
    /// Scroll the view by a number of pages (positive is up).
    ScrollPages(i32),
}

/// Interprets a key press. Returns `None` for keys that do nothing.
pub fn key_press(
    key: &WindowKey,
    modifiers: WindowModifiers,
    text: Option<&str>,
    mode: TerminalMode,
) -> Option<KeyAction> {
    if let Some(shortcut) = shortcut(key, modifiers) {
        return Some(shortcut);
    }

    let terminal_modifiers = Modifiers {
        shift: modifiers.shift(),
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
    };
    // Ctrl+Alt is how AltGr is reported on Windows; it produces printable
    // characters such as `@` or `\` on many layouts, so prefer its text.
    let alt_gr = modifiers.control() && modifiers.alt();

    let terminal_key = match key {
        WindowKey::Named(named) => named_key(*named),
        WindowKey::Character(chars) if (modifiers.control() || modifiers.alt()) && !alt_gr => {
            chars.chars().next().map(Key::Char)
        }
        _ => None,
    };

    if let Some(bytes) = terminal_key
        .and_then(|terminal_key| input::encode_key(terminal_key, terminal_modifiers, mode))
    {
        return Some(KeyAction::Input(bytes));
    }

    text.filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
        .map(|text| KeyAction::Input(text.as_bytes().to_vec()))
}

fn shortcut(key: &WindowKey, modifiers: WindowModifiers) -> Option<KeyAction> {
    let character = match key {
        WindowKey::Character(chars) => chars.chars().next().map(|ch| ch.to_ascii_lowercase()),
        _ => None,
    };
    let command = modifiers.logo() || (modifiers.control() && modifiers.shift());

    match (key, character) {
        (_, Some('v')) if command => Some(KeyAction::Paste),
        (WindowKey::Named(Named::Insert), _) if modifiers.shift() => Some(KeyAction::Paste),
        (WindowKey::Named(Named::PageUp), _) if modifiers.shift() => {
            Some(KeyAction::ScrollPages(1))
        }
        (WindowKey::Named(Named::PageDown), _) if modifiers.shift() => {
            Some(KeyAction::ScrollPages(-1))
        }
        _ => None,
    }
}

fn named_key(named: Named) -> Option<Key> {
    Some(match named {
        Named::Enter => Key::Enter,
        Named::Backspace => Key::Backspace,
        Named::Tab => Key::Tab,
        Named::Escape => Key::Escape,
        Named::ArrowUp => Key::Up,
        Named::ArrowDown => Key::Down,
        Named::ArrowLeft => Key::Left,
        Named::ArrowRight => Key::Right,
        Named::Home => Key::Home,
        Named::End => Key::End,
        Named::PageUp => Key::PageUp,
        Named::PageDown => Key::PageDown,
        Named::Insert => Key::Insert,
        Named::Delete => Key::Delete,
        Named::Space => Key::Char(' '),
        Named::F1 => Key::F(1),
        Named::F2 => Key::F(2),
        Named::F3 => Key::F(3),
        Named::F4 => Key::F(4),
        Named::F5 => Key::F(5),
        Named::F6 => Key::F(6),
        Named::F7 => Key::F(7),
        Named::F8 => Key::F(8),
        Named::F9 => Key::F(9),
        Named::F10 => Key::F(10),
        Named::F11 => Key::F(11),
        Named::F12 => Key::F(12),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: WindowKey, modifiers: WindowModifiers, text: Option<&str>) -> Option<KeyAction> {
        key_press(&key, modifiers, text, TerminalMode::default())
    }

    fn character(ch: &str) -> WindowKey {
        WindowKey::Character(ch.into())
    }

    fn input(bytes: &[u8]) -> Option<KeyAction> {
        Some(KeyAction::Input(bytes.to_vec()))
    }

    #[test]
    fn printable_text_is_sent_as_is() {
        assert_eq!(
            press(character("a"), WindowModifiers::empty(), Some("a")),
            input(b"a")
        );
        assert_eq!(
            press(character("ñ"), WindowModifiers::empty(), Some("ñ")),
            input("ñ".as_bytes())
        );
    }

    #[test]
    fn named_keys_produce_sequences() {
        let none = WindowModifiers::empty();
        assert_eq!(
            press(WindowKey::Named(Named::Enter), none, Some("\r")),
            input(b"\r")
        );
        assert_eq!(
            press(WindowKey::Named(Named::ArrowUp), none, None),
            input(b"\x1b[A")
        );
        assert_eq!(
            press(WindowKey::Named(Named::Space), none, Some(" ")),
            input(b" ")
        );
    }

    #[test]
    fn ctrl_letters_produce_control_codes() {
        assert_eq!(
            press(character("c"), WindowModifiers::CTRL, None),
            input(&[0x03])
        );
    }

    #[test]
    fn alt_gr_keeps_the_typed_character() {
        let alt_gr = WindowModifiers::CTRL | WindowModifiers::ALT;
        assert_eq!(press(character("q"), alt_gr, Some("@")), input(b"@"));
    }

    #[test]
    fn paste_shortcuts() {
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        assert_eq!(
            press(character("V"), ctrl_shift, None),
            Some(KeyAction::Paste)
        );
        assert_eq!(
            press(character("v"), WindowModifiers::LOGO, None),
            Some(KeyAction::Paste)
        );
        assert_eq!(
            press(
                WindowKey::Named(Named::Insert),
                WindowModifiers::SHIFT,
                None
            ),
            Some(KeyAction::Paste)
        );
    }

    #[test]
    fn shift_page_keys_scroll_history() {
        assert_eq!(
            press(
                WindowKey::Named(Named::PageUp),
                WindowModifiers::SHIFT,
                None
            ),
            Some(KeyAction::ScrollPages(1))
        );
    }

    #[test]
    fn modifier_keys_alone_do_nothing() {
        assert_eq!(
            press(WindowKey::Named(Named::Shift), WindowModifiers::SHIFT, None),
            None
        );
    }
}
