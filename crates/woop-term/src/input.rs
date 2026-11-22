//! Translation of keyboard input into the bytes a terminal application expects.

use crate::terminal::TerminalMode;

/// A key that produces a control sequence rather than plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Enter / Return.
    Enter,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert.
    Insert,
    /// Delete.
    Delete,
    /// Function key F1 to F12.
    F(u8),
    /// A character key, used when combined with Ctrl or Alt.
    Char(char),
}

/// Modifier keys held during a key press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
}

impl Modifiers {
    /// The xterm modifier parameter: 1 plus a bit per modifier.
    fn parameter(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// Encodes a key press, or returns `None` when the key produces no input
/// (the caller should then fall back to the text the key produced, if any).
pub fn encode_key(key: Key, modifiers: Modifiers, mode: TerminalMode) -> Option<Vec<u8>> {
    let alt_prefix = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if modifiers.alt {
            out.push(0x1b);
        }
        out.extend_from_slice(bytes);
        out
    };
    let parameter = modifiers.parameter();

    let bytes = match key {
        Key::Enter => alt_prefix(b"\r"),
        Key::Backspace if modifiers.ctrl => alt_prefix(b"\x08"),
        Key::Backspace => alt_prefix(b"\x7f"),
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt_prefix(b"\t"),
        Key::Escape => alt_prefix(b"\x1b"),
        Key::Up => cursor_key(b'A', parameter, mode),
        Key::Down => cursor_key(b'B', parameter, mode),
        Key::Right => cursor_key(b'C', parameter, mode),
        Key::Left => cursor_key(b'D', parameter, mode),
        Key::Home => cursor_key(b'H', parameter, mode),
        Key::End => cursor_key(b'F', parameter, mode),
        Key::Insert => tilde_key(2, parameter),
        Key::Delete => tilde_key(3, parameter),
        Key::PageUp => tilde_key(5, parameter),
        Key::PageDown => tilde_key(6, parameter),
        Key::F(number @ 1..=4) => {
            let letter = b'P' + (number - 1);
            if parameter > 1 {
                format!("\x1b[1;{parameter}{}", char::from(letter)).into_bytes()
            } else {
                vec![0x1b, b'O', letter]
            }
        }
        Key::F(number @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(number - 5)];
            tilde_key(code, parameter)
        }
        Key::F(_) => return None,
        Key::Char(ch) if modifiers.ctrl => alt_prefix(&[control_byte(ch)?]),
        Key::Char(ch) if modifiers.alt => {
            let mut buffer = [0u8; 4];
            alt_prefix(ch.encode_utf8(&mut buffer).as_bytes())
        }
        Key::Char(_) => return None,
    };
    Some(bytes)
}

/// Encodes pasted text, wrapping it in bracketed paste markers when the
/// application requested them.
///
/// Control characters other than tab and line breaks are removed: copied
/// text (for example from a web page) can hide escape sequences or keys
/// such as Ctrl+C, which would otherwise act as typed keys or end a
/// bracketed paste early.
pub fn encode_paste(text: &str, mode: TerminalMode) -> Vec<u8> {
    let text: String = text
        .chars()
        .filter(|&ch| !ch.is_control() || matches!(ch, '\t' | '\r' | '\n'))
        .collect();
    if mode.bracketed_paste {
        let mut out = Vec::with_capacity(text.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

fn cursor_key(letter: u8, parameter: u8, mode: TerminalMode) -> Vec<u8> {
    if parameter > 1 {
        format!("\x1b[1;{parameter}{}", char::from(letter)).into_bytes()
    } else if mode.app_cursor {
        vec![0x1b, b'O', letter]
    } else {
        vec![0x1b, b'[', letter]
    }
}

fn tilde_key(code: u8, parameter: u8) -> Vec<u8> {
    if parameter > 1 {
        format!("\x1b[{code};{parameter}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn control_byte(ch: char) -> Option<u8> {
    match ch.to_ascii_lowercase() {
        ch @ 'a'..='z' => Some(ch as u8 & 0x1f),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '/' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
    };
    const ALT: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: true,
    };

    fn normal() -> TerminalMode {
        TerminalMode::default()
    }

    fn encode(key: Key, modifiers: Modifiers) -> Vec<u8> {
        encode_key(key, modifiers, normal()).expect("key produces input")
    }

    #[test]
    fn basic_keys() {
        assert_eq!(encode(Key::Enter, NONE), b"\r");
        assert_eq!(encode(Key::Backspace, NONE), b"\x7f");
        assert_eq!(encode(Key::Tab, NONE), b"\t");
        assert_eq!(encode(Key::Escape, NONE), b"\x1b");
    }

    #[test]
    fn control_characters() {
        assert_eq!(encode(Key::Char('c'), CTRL), [0x03]);
        assert_eq!(encode(Key::Char('D'), CTRL), [0x04]);
        assert_eq!(encode(Key::Char('['), CTRL), [0x1b]);
        assert_eq!(encode_key(Key::Char('1'), CTRL, normal()), None);
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(encode(Key::Char('b'), ALT), b"\x1bb");
        assert_eq!(encode(Key::Enter, ALT), b"\x1b\r");
    }

    #[test]
    fn plain_characters_are_left_to_text_input() {
        assert_eq!(encode_key(Key::Char('a'), NONE, normal()), None);
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(encode(Key::Up, NONE), b"\x1b[A");
        let app = TerminalMode {
            app_cursor: true,
            ..TerminalMode::default()
        };
        assert_eq!(
            encode_key(Key::Up, NONE, app).as_deref(),
            Some(&b"\x1bOA"[..])
        );
    }

    #[test]
    fn modified_arrows_use_parameters() {
        assert_eq!(encode(Key::Right, CTRL), b"\x1b[1;5C");
        let shift = Modifiers {
            shift: true,
            ..NONE
        };
        assert_eq!(encode(Key::Left, shift), b"\x1b[1;2D");
    }

    #[test]
    fn editing_and_function_keys() {
        assert_eq!(encode(Key::Delete, NONE), b"\x1b[3~");
        assert_eq!(encode(Key::PageUp, NONE), b"\x1b[5~");
        assert_eq!(encode(Key::F(1), NONE), b"\x1bOP");
        assert_eq!(encode(Key::F(5), NONE), b"\x1b[15~");
        assert_eq!(encode(Key::F(12), CTRL), b"\x1b[24;5~");
    }

    #[test]
    fn paste_normalises_line_endings() {
        assert_eq!(encode_paste("a\r\nb\nc", normal()), b"a\rb\rc");
    }

    #[test]
    fn pasted_control_characters_are_removed() {
        // Text copied from a web page can hide keys: Ctrl+U clears the
        // line, ESC starts a key sequence, Ctrl+C interrupts.
        assert_eq!(
            encode_paste("echo safe\x15\x1b[A\x03\u{9b}\x7f\tok", normal()),
            b"echo safe[A\tok"
        );
    }

    #[test]
    fn bracketed_paste_wraps_and_sanitises() {
        let mode = TerminalMode {
            bracketed_paste: true,
            ..TerminalMode::default()
        };
        assert_eq!(
            encode_paste("ls\x1b[201~rm", mode),
            b"\x1b[200~ls[201~rm\x1b[201~"
        );
    }
}
