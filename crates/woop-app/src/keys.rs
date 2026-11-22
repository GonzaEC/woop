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
    /// Copy the selected text to the clipboard.
    Copy,
    /// Scroll the view by a number of pages (positive is up).
    ScrollPages(i32),
    /// Move the view to the previous (negative) or next command block.
    JumpBlock(i32),
}

/// Clipboard shortcut conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcuts {
    /// Ctrl+C copies when text is selected and interrupts otherwise;
    /// Ctrl+V pastes. Ctrl+Shift+C and Ctrl+Shift+V also work.
    Windows,
    /// Ctrl+Shift+C and Ctrl+Shift+V, leaving Ctrl+C and Ctrl+V to the shell.
    Unix,
    /// Cmd+C and Cmd+V.
    MacOs,
}

impl Shortcuts {
    /// Conventions of the platform Woop was built for.
    pub const fn platform() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unix
        }
    }
}

/// Context needed to interpret a key press.
#[derive(Debug, Clone, Copy)]
pub struct KeyContext {
    /// Input modes of the running application.
    pub mode: TerminalMode,
    /// Some text is selected.
    pub has_selection: bool,
    /// Clipboard shortcut conventions.
    pub shortcuts: Shortcuts,
    /// Woop's command editor takes the keys; only Woop's own view shortcuts
    /// (jumping between blocks, scrolling pages) are left to the grid.
    pub editor_active: bool,
}

/// Interprets a key press on the grid. Returns `None` for keys that do
/// nothing there.
pub fn key_press(
    key: &WindowKey,
    modifiers: WindowModifiers,
    text: Option<&str>,
    context: KeyContext,
) -> Option<KeyAction> {
    let mode = context.mode;
    if context.editor_active {
        return shortcut(key, modifiers, context).filter(|action| {
            matches!(action, KeyAction::JumpBlock(_) | KeyAction::ScrollPages(_))
        });
    }
    if let Some(shortcut) = shortcut(key, modifiers, context) {
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

/// What a key press means in Woop's command editor (ADR 0016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorKey {
    /// Let the editor handle it as text editing (typing, moving, selecting,
    /// its own copy, cut, paste and select all).
    Edit,
    /// Not for the editor: left to the grid's shortcuts, or ignored.
    Ignore,
    /// Run the command.
    Submit,
    /// Break the line.
    Newline,
    /// Show the previous command of the history.
    HistoryPrevious,
    /// Show the next command of the history.
    HistoryNext,
    /// Copy the editor's selection.
    Copy,
    /// Copy the grid's selection.
    CopyGrid,
    /// Paste the clipboard into the editor.
    Paste,
    /// Clear the editor.
    Clear,
    /// Send these bytes to the shell, whose line is empty.
    Shell(Vec<u8>),
    /// Type the editor's text into the shell's line, then these bytes, and
    /// leave the rest of the prompt to the shell's own line editor.
    HandOff(Vec<u8>),
}

/// State of the command editor needed to interpret a key press.
#[derive(Debug, Clone, Copy)]
pub struct EditorContext {
    /// The grid's context: modes, shortcuts, and whether the grid has a
    /// selection.
    pub keys: KeyContext,
    /// The editor has a selection.
    pub has_selection: bool,
    /// The editor holds no text.
    pub is_empty: bool,
    /// The cursor is on the editor's first line.
    pub on_first_line: bool,
    /// The cursor is on the editor's last line.
    pub on_last_line: bool,
}

/// Interprets a key press in the command editor.
pub fn editor_key(
    key: &WindowKey,
    modifiers: WindowModifiers,
    context: EditorContext,
) -> EditorKey {
    let keys = KeyContext {
        has_selection: context.has_selection || context.keys.has_selection,
        editor_active: false,
        ..context.keys
    };
    match shortcut(key, modifiers, keys) {
        Some(KeyAction::Copy) if context.has_selection => return EditorKey::Copy,
        Some(KeyAction::Copy) if context.keys.has_selection => return EditorKey::CopyGrid,
        Some(KeyAction::Copy) => return EditorKey::Ignore,
        Some(KeyAction::Paste) => return EditorKey::Paste,
        Some(KeyAction::JumpBlock(_) | KeyAction::ScrollPages(_)) => return EditorKey::Ignore,
        Some(KeyAction::Input(_)) | None => {}
    }

    let plain = !modifiers.control() && !modifiers.alt() && !modifiers.logo();
    let ctrl_only =
        modifiers.control() && !modifiers.shift() && !modifiers.alt() && !modifiers.logo();
    let letter = match key {
        WindowKey::Character(chars) => chars.chars().next().map(|ch| ch.to_ascii_lowercase()),
        _ => None,
    };
    let terminal_modifiers = Modifiers {
        shift: modifiers.shift(),
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
    };

    match (key, letter) {
        (WindowKey::Named(Named::Enter), _) if plain && modifiers.shift() => EditorKey::Newline,
        (WindowKey::Named(Named::Enter), _) if plain => EditorKey::Submit,
        (WindowKey::Named(Named::ArrowUp), _)
            if plain && !modifiers.shift() && context.on_first_line =>
        {
            EditorKey::HistoryPrevious
        }
        (WindowKey::Named(Named::ArrowDown), _)
            if plain && !modifiers.shift() && context.on_last_line =>
        {
            EditorKey::HistoryNext
        }
        // The shell's completion and history search need the text in its
        // own line.
        (WindowKey::Named(Named::Tab), _) if plain => {
            input::encode_key(Key::Tab, terminal_modifiers, context.keys.mode)
                .map_or(EditorKey::Ignore, EditorKey::HandOff)
        }
        (_, Some('r')) if ctrl_only => EditorKey::HandOff(vec![0x12]),
        // The shell does not see keys while the editor is active, so Ctrl+C
        // copies on every platform when something is selected. Otherwise
        // it discards the text, and interrupts at an empty prompt.
        (_, Some('c')) if ctrl_only && context.has_selection => EditorKey::Copy,
        (_, Some('c')) if ctrl_only && context.keys.has_selection => EditorKey::CopyGrid,
        (_, Some('c')) if ctrl_only && context.is_empty => EditorKey::Shell(vec![0x03]),
        (_, Some('c')) if ctrl_only => EditorKey::Clear,
        // The editor's own shortcuts (Cmd on macOS, where Ctrl+A and the
        // like keep their meaning for the shell).
        (_, Some('a' | 'x' | 'v')) if ctrl_only && context.keys.shortcuts != Shortcuts::MacOs => {
            EditorKey::Edit
        }
        // Ending the shell would lose the text being written.
        (_, Some('d')) if ctrl_only && !context.is_empty => EditorKey::Ignore,
        // Other control keys act as at an empty prompt (Ctrl+L clears the
        // screen, Ctrl+D ends the shell).
        (WindowKey::Character(_), Some(ch)) if ctrl_only => {
            input::encode_key(Key::Char(ch), terminal_modifiers, context.keys.mode)
                .map_or(EditorKey::Ignore, EditorKey::Shell)
        }
        (WindowKey::Named(Named::Escape), _) => EditorKey::Ignore,
        (WindowKey::Named(named), _) if matches!(named_key(*named), Some(Key::F(_))) => {
            named_key(*named)
                .and_then(|key| input::encode_key(key, terminal_modifiers, context.keys.mode))
                .map_or(EditorKey::Ignore, EditorKey::Shell)
        }
        _ => EditorKey::Edit,
    }
}

fn shortcut(key: &WindowKey, modifiers: WindowModifiers, context: KeyContext) -> Option<KeyAction> {
    let character = match key {
        WindowKey::Character(chars) => chars.chars().next().map(|ch| ch.to_ascii_lowercase()),
        _ => None,
    };
    let ctrl_only =
        modifiers.control() && !modifiers.shift() && !modifiers.alt() && !modifiers.logo();
    let ctrl_shift = modifiers.control() && modifiers.shift() && !modifiers.alt();
    let (copy, paste) = match context.shortcuts {
        Shortcuts::Windows => (
            ctrl_shift || (ctrl_only && context.has_selection),
            ctrl_shift || ctrl_only,
        ),
        Shortcuts::Unix => (ctrl_shift, ctrl_shift),
        Shortcuts::MacOs => (modifiers.logo(), modifiers.logo()),
    };
    // Jumping between blocks: Cmd+Up/Down on macOS, Ctrl+Shift+Up/Down
    // elsewhere (Ctrl+Up/Down alone are used by line editors).
    let jump = match context.shortcuts {
        Shortcuts::MacOs => modifiers.logo() && !modifiers.shift(),
        Shortcuts::Windows | Shortcuts::Unix => ctrl_shift,
    };

    match (key, character) {
        (_, Some('c')) if copy => Some(KeyAction::Copy),
        (_, Some('v')) if paste => Some(KeyAction::Paste),
        (WindowKey::Named(Named::Insert), _) if modifiers.shift() => Some(KeyAction::Paste),
        (WindowKey::Named(Named::ArrowUp), _) if jump => Some(KeyAction::JumpBlock(-1)),
        (WindowKey::Named(Named::ArrowDown), _) if jump => Some(KeyAction::JumpBlock(1)),
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
        let context = KeyContext {
            mode: TerminalMode::default(),
            has_selection: false,
            shortcuts: Shortcuts::Unix,
            editor_active: false,
        };
        key_press(&key, modifiers, text, context)
    }

    fn press_with(
        shortcuts: Shortcuts,
        has_selection: bool,
        key: WindowKey,
        modifiers: WindowModifiers,
    ) -> Option<KeyAction> {
        let context = KeyContext {
            mode: TerminalMode::default(),
            has_selection,
            shortcuts,
            editor_active: false,
        };
        key_press(&key, modifiers, None, context)
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
        let v = || character("v");
        assert_eq!(
            press_with(Shortcuts::Unix, false, v(), ctrl_shift),
            Some(KeyAction::Paste)
        );
        assert_eq!(
            press_with(Shortcuts::Windows, false, v(), WindowModifiers::CTRL),
            Some(KeyAction::Paste)
        );
        assert_eq!(
            press_with(Shortcuts::MacOs, false, v(), WindowModifiers::LOGO),
            Some(KeyAction::Paste)
        );
        assert_eq!(
            press_with(Shortcuts::Unix, false, v(), WindowModifiers::CTRL),
            input(&[0x16]),
            "Ctrl+V reaches the shell outside Windows"
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
    fn ctrl_c_copies_on_windows_only_while_text_is_selected() {
        let c = || character("c");
        let ctrl = WindowModifiers::CTRL;
        assert_eq!(
            press_with(Shortcuts::Windows, true, c(), ctrl),
            Some(KeyAction::Copy)
        );
        assert_eq!(
            press_with(Shortcuts::Windows, false, c(), ctrl),
            input(&[0x03])
        );
        assert_eq!(press_with(Shortcuts::Unix, true, c(), ctrl), input(&[0x03]));
    }

    #[test]
    fn block_jump_shortcuts() {
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        let up = || WindowKey::Named(Named::ArrowUp);
        let down = || WindowKey::Named(Named::ArrowDown);
        assert_eq!(
            press_with(Shortcuts::Windows, false, up(), ctrl_shift),
            Some(KeyAction::JumpBlock(-1))
        );
        assert_eq!(
            press_with(Shortcuts::Unix, false, down(), ctrl_shift),
            Some(KeyAction::JumpBlock(1))
        );
        assert_eq!(
            press_with(Shortcuts::MacOs, false, up(), WindowModifiers::LOGO),
            Some(KeyAction::JumpBlock(-1))
        );
        // Ctrl+Up alone still reaches the shell.
        assert_eq!(
            press_with(Shortcuts::Windows, false, up(), WindowModifiers::CTRL),
            input(b"\x1b[1;5A")
        );
    }

    #[test]
    fn copy_shortcuts() {
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        assert_eq!(
            press_with(Shortcuts::Unix, true, character("C"), ctrl_shift),
            Some(KeyAction::Copy)
        );
        assert_eq!(
            press_with(Shortcuts::Windows, true, character("C"), ctrl_shift),
            Some(KeyAction::Copy)
        );
        assert_eq!(
            press_with(
                Shortcuts::MacOs,
                true,
                character("c"),
                WindowModifiers::LOGO
            ),
            Some(KeyAction::Copy)
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

    #[test]
    fn the_grid_only_keeps_view_shortcuts_while_the_editor_is_active() {
        let context = KeyContext {
            mode: TerminalMode::default(),
            has_selection: true,
            shortcuts: Shortcuts::Windows,
            editor_active: true,
        };
        let grid = |key: WindowKey, modifiers| key_press(&key, modifiers, Some("a"), context);
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        assert_eq!(grid(character("a"), WindowModifiers::empty()), None);
        assert_eq!(grid(character("c"), WindowModifiers::CTRL), None);
        assert_eq!(grid(character("v"), WindowModifiers::CTRL), None);
        assert_eq!(
            grid(WindowKey::Named(Named::Enter), WindowModifiers::empty()),
            None
        );
        assert_eq!(
            grid(WindowKey::Named(Named::ArrowUp), ctrl_shift),
            Some(KeyAction::JumpBlock(-1))
        );
        assert_eq!(
            grid(WindowKey::Named(Named::PageUp), WindowModifiers::SHIFT),
            Some(KeyAction::ScrollPages(1))
        );
    }

    #[derive(Clone, Copy)]
    struct Editor {
        shortcuts: Shortcuts,
        has_selection: bool,
        grid_selection: bool,
        is_empty: bool,
        on_first_line: bool,
        on_last_line: bool,
        bracketed_paste: bool,
    }

    const EDITOR: Editor = Editor {
        shortcuts: Shortcuts::Windows,
        has_selection: false,
        grid_selection: false,
        is_empty: false,
        on_first_line: true,
        on_last_line: true,
        bracketed_paste: false,
    };

    impl Editor {
        fn key(self, key: WindowKey, modifiers: WindowModifiers) -> EditorKey {
            let context = EditorContext {
                keys: KeyContext {
                    mode: TerminalMode {
                        bracketed_paste: self.bracketed_paste,
                        ..TerminalMode::default()
                    },
                    has_selection: self.grid_selection,
                    shortcuts: self.shortcuts,
                    editor_active: true,
                },
                has_selection: self.has_selection,
                is_empty: self.is_empty,
                on_first_line: self.on_first_line,
                on_last_line: self.on_last_line,
            };
            editor_key(&key, modifiers, context)
        }
    }

    fn named(named: Named) -> WindowKey {
        WindowKey::Named(named)
    }

    #[test]
    fn enter_submits_and_shift_enter_breaks_the_line() {
        let none = WindowModifiers::empty();
        assert_eq!(EDITOR.key(named(Named::Enter), none), EditorKey::Submit);
        assert_eq!(
            EDITOR.key(named(Named::Enter), WindowModifiers::SHIFT),
            EditorKey::Newline
        );
    }

    #[test]
    fn arrows_walk_the_history_only_at_the_edges() {
        let none = WindowModifiers::empty();
        assert_eq!(
            EDITOR.key(named(Named::ArrowUp), none),
            EditorKey::HistoryPrevious
        );
        assert_eq!(
            EDITOR.key(named(Named::ArrowDown), none),
            EditorKey::HistoryNext
        );
        let middle = Editor {
            on_first_line: false,
            on_last_line: false,
            ..EDITOR
        };
        assert_eq!(middle.key(named(Named::ArrowUp), none), EditorKey::Edit);
        assert_eq!(middle.key(named(Named::ArrowDown), none), EditorKey::Edit);
        // Shift+Up selects instead.
        assert_eq!(
            EDITOR.key(named(Named::ArrowUp), WindowModifiers::SHIFT),
            EditorKey::Edit
        );
    }

    #[test]
    fn tab_and_ctrl_r_hand_off_to_the_shell() {
        assert_eq!(
            EDITOR.key(named(Named::Tab), WindowModifiers::empty()),
            EditorKey::HandOff(b"\t".to_vec())
        );
        assert_eq!(
            EDITOR.key(named(Named::Tab), WindowModifiers::SHIFT),
            EditorKey::HandOff(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            EDITOR.key(character("r"), WindowModifiers::CTRL),
            EditorKey::HandOff(vec![0x12])
        );
    }

    #[test]
    fn ctrl_c_copies_then_clears_then_interrupts() {
        let ctrl = WindowModifiers::CTRL;
        for shortcuts in [Shortcuts::Windows, Shortcuts::Unix, Shortcuts::MacOs] {
            let editor = Editor {
                shortcuts,
                ..EDITOR
            };
            let selected = Editor {
                has_selection: true,
                grid_selection: true,
                ..editor
            };
            assert_eq!(selected.key(character("c"), ctrl), EditorKey::Copy);
            let grid = Editor {
                grid_selection: true,
                ..editor
            };
            assert_eq!(grid.key(character("c"), ctrl), EditorKey::CopyGrid);
            assert_eq!(editor.key(character("c"), ctrl), EditorKey::Clear);
            let empty = Editor {
                is_empty: true,
                ..editor
            };
            assert_eq!(
                empty.key(character("c"), ctrl),
                EditorKey::Shell(vec![0x03])
            );
        }
    }

    #[test]
    fn platform_copy_and_paste_shortcuts_reach_the_editor() {
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        let unix = Editor {
            shortcuts: Shortcuts::Unix,
            has_selection: true,
            ..EDITOR
        };
        assert_eq!(unix.key(character("C"), ctrl_shift), EditorKey::Copy);
        assert_eq!(unix.key(character("V"), ctrl_shift), EditorKey::Paste);
        assert_eq!(
            EDITOR.key(character("v"), WindowModifiers::CTRL),
            EditorKey::Paste
        );
        assert_eq!(
            EDITOR.key(named(Named::Insert), WindowModifiers::SHIFT),
            EditorKey::Paste
        );
        let mac = Editor {
            shortcuts: Shortcuts::MacOs,
            ..EDITOR
        };
        assert_eq!(
            mac.key(character("v"), WindowModifiers::LOGO),
            EditorKey::Paste
        );
        assert_eq!(
            mac.key(character("a"), WindowModifiers::LOGO),
            EditorKey::Edit
        );
        // Ctrl+A is not an editing shortcut on macOS.
        assert_eq!(
            mac.key(character("a"), WindowModifiers::CTRL),
            EditorKey::Shell(vec![0x01])
        );
        // Nothing selected anywhere: the copy shortcut does nothing.
        assert_eq!(
            Editor {
                shortcuts: Shortcuts::Unix,
                ..EDITOR
            }
            .key(character("C"), ctrl_shift),
            EditorKey::Ignore
        );
    }

    #[test]
    fn editing_shortcuts_stay_in_the_editor() {
        let ctrl = WindowModifiers::CTRL;
        assert_eq!(EDITOR.key(character("a"), ctrl), EditorKey::Edit);
        assert_eq!(EDITOR.key(character("x"), ctrl), EditorKey::Edit);
        assert_eq!(
            EDITOR.key(character("a"), WindowModifiers::empty()),
            EditorKey::Edit
        );
        assert_eq!(EDITOR.key(named(Named::ArrowLeft), ctrl), EditorKey::Edit);
        // AltGr produces characters.
        assert_eq!(
            EDITOR.key(character("q"), WindowModifiers::CTRL | WindowModifiers::ALT),
            EditorKey::Edit
        );
        assert_eq!(
            EDITOR.key(named(Named::Escape), WindowModifiers::empty()),
            EditorKey::Ignore
        );
    }

    #[test]
    fn view_shortcuts_are_left_to_the_grid() {
        let ctrl_shift = WindowModifiers::CTRL | WindowModifiers::SHIFT;
        assert_eq!(
            EDITOR.key(named(Named::ArrowUp), ctrl_shift),
            EditorKey::Ignore
        );
        assert_eq!(
            EDITOR.key(named(Named::PageDown), WindowModifiers::SHIFT),
            EditorKey::Ignore
        );
    }

    #[test]
    fn other_control_keys_reach_the_empty_shell_line() {
        let ctrl = WindowModifiers::CTRL;
        assert_eq!(
            EDITOR.key(character("l"), ctrl),
            EditorKey::Shell(vec![0x0c])
        );
        // Ctrl+D ends the shell only from an empty editor.
        assert_eq!(EDITOR.key(character("d"), ctrl), EditorKey::Ignore);
        let empty = Editor {
            is_empty: true,
            ..EDITOR
        };
        assert_eq!(
            empty.key(character("d"), ctrl),
            EditorKey::Shell(vec![0x04])
        );
        assert_eq!(
            EDITOR.key(named(Named::F5), WindowModifiers::empty()),
            EditorKey::Shell(b"\x1b[15~".to_vec())
        );
        // Unused: does not become a control code.
        assert_eq!(EDITOR.key(character("1"), ctrl), EditorKey::Ignore);
    }
}
