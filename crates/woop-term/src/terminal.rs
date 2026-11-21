//! Terminal state: escape sequence parsing and the screen grid.

use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::Processor;

use crate::screen::Screen;

/// Size of the terminal screen, in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    /// Number of visible rows.
    pub rows: u16,
    /// Number of visible columns.
    pub cols: u16,
}

impl TerminalSize {
    /// Creates a size of `rows` by `cols` cells.
    ///
    /// Both dimensions are clamped to at least one cell.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.max(1),
            cols: cols.max(1),
        }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

impl From<TerminalSize> for woop_pty::PtySize {
    fn from(size: TerminalSize) -> Self {
        Self::new(size.rows, size.cols)
    }
}

/// Position of the cursor on the visible screen, zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    /// Row, counted from the top of the screen.
    pub row: usize,
    /// Column, counted from the left edge.
    pub col: usize,
}

/// Notifications produced while processing terminal output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The application changed the window title.
    Title(String),
    /// The application reset the window title to its default.
    ResetTitle,
    /// The application rang the bell.
    Bell,
    /// The application asked to copy text to the clipboard.
    ClipboardStore(String),
    /// Bytes that must be written back to the process, such as the answer
    /// to a cursor position query.
    Reply(String),
}

/// Forwards events from the emulator to the owning [`Terminal`].
#[derive(Clone)]
pub(crate) struct EventProxy {
    sender: Sender<TerminalEvent>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let event = match event {
            Event::Title(title) => TerminalEvent::Title(title),
            Event::ResetTitle => TerminalEvent::ResetTitle,
            Event::Bell => TerminalEvent::Bell,
            Event::ClipboardStore(_, text) => TerminalEvent::ClipboardStore(text),
            Event::PtyWrite(text) => TerminalEvent::Reply(text),
            _ => return,
        };
        // The receiver lives as long as the terminal, so a failed send only
        // happens during teardown and can be ignored.
        let _ = self.sender.send(event);
    }
}

/// Emulated terminal screen.
///
/// Feed process output with [`Terminal::advance`] and read the resulting
/// screen with [`Terminal::screen_lines`] or [`Terminal::screen_text`].
pub struct Terminal {
    term: Term<EventProxy>,
    parser: Processor,
    events: Receiver<TerminalEvent>,
    size: TerminalSize,
}

impl Terminal {
    /// Creates an empty terminal of the given size.
    pub fn new(size: TerminalSize) -> Self {
        let (sender, events) = mpsc::channel();
        let term = Term::new(Config::default(), &size, EventProxy { sender });
        Self {
            term,
            parser: Processor::new(),
            events,
            size,
        }
    }

    /// Processes output produced by the process running in the terminal.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Changes the screen size, reflowing content as needed.
    pub fn resize(&mut self, size: TerminalSize) {
        if size != self.size {
            self.term.resize(size);
            self.size = size;
        }
    }

    /// Returns the current screen size.
    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// Returns and clears the events produced since the last call.
    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        self.events.try_iter().collect()
    }

    /// Returns the cursor position on the visible screen.
    pub fn cursor(&self) -> CursorPosition {
        let point = self.term.grid().cursor.point;
        CursorPosition {
            row: usize::try_from(point.line.0).unwrap_or(0),
            col: point.column.0,
        }
    }

    /// Returns the visible screen as one string per row, without trailing
    /// whitespace.
    pub fn screen_lines(&self) -> Vec<String> {
        let grid = self.term.grid();
        (0..grid.screen_lines())
            .map(|row| {
                let line = &grid[Line(row as i32)];
                let mut text = String::with_capacity(grid.columns());
                for col in 0..grid.columns() {
                    let cell = &line[Column(col)];
                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    text.push(cell.c);
                    if let Some(zero_width) = cell.zerowidth() {
                        text.extend(zero_width);
                    }
                }
                text.truncate(text.trim_end().len());
                text
            })
            .collect()
    }

    /// Returns the visible screen as text, with trailing empty rows removed.
    pub fn screen_text(&self) -> String {
        let mut lines = self.screen_lines();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// Returns a snapshot of the visible screen, for rendering.
    pub fn screen(&self) -> Screen {
        Screen::from_content(
            self.term.renderable_content(),
            usize::from(self.size.rows),
            usize::from(self.size.cols),
        )
    }

    /// Returns the input modes requested by the running application.
    pub fn mode(&self) -> TerminalMode {
        let mode = self.term.mode();
        TerminalMode {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
        }
    }

    /// Scrolls the view through history. Positive values scroll up, towards
    /// older output.
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// Scrolls the view back to the most recent output.
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// Returns how many lines the view is scrolled back into history.
    pub fn scroll_offset(&self) -> usize {
        self.term.grid().display_offset()
    }
}

/// Input modes requested by the application running in the terminal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalMode {
    /// Cursor keys send application sequences (`ESC O A` instead of `ESC [ A`).
    pub app_cursor: bool,
    /// Pasted text must be wrapped in bracketed paste markers.
    pub bracketed_paste: bool,
    /// The alternate screen is active, as in full-screen applications.
    pub alt_screen: bool,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("size", &self.size)
            .field("cursor", &self.cursor())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(rows: u16, cols: u16) -> Terminal {
        Terminal::new(TerminalSize::new(rows, cols))
    }

    #[test]
    fn writes_plain_text() {
        let mut term = terminal(4, 20);
        term.advance(b"hello\r\nworld");

        assert_eq!(term.screen_text(), "hello\nworld");
        assert_eq!(term.cursor(), CursorPosition { row: 1, col: 5 });
    }

    #[test]
    fn ignores_color_sequences_in_text() {
        let mut term = terminal(2, 20);
        term.advance(b"\x1b[1;31mred\x1b[0m plain");

        assert_eq!(term.screen_lines()[0], "red plain");
    }

    #[test]
    fn wraps_long_lines() {
        let mut term = terminal(3, 5);
        term.advance(b"abcdefgh");

        assert_eq!(term.screen_lines(), ["abcde", "fgh", ""]);
    }

    #[test]
    fn clears_screen_and_moves_cursor() {
        let mut term = terminal(3, 10);
        term.advance(b"old text\x1b[2J\x1b[2;3Hnew");

        assert_eq!(term.screen_lines(), ["", "  new", ""]);
        assert_eq!(term.cursor(), CursorPosition { row: 1, col: 5 });
    }

    #[test]
    fn handles_wide_characters() {
        let mut term = terminal(1, 10);
        term.advance("日本".as_bytes());

        assert_eq!(term.screen_lines()[0], "日本");
        assert_eq!(term.cursor().col, 4);
    }

    #[test]
    fn reports_title_changes() {
        let mut term = terminal(2, 10);
        term.advance(b"\x1b]0;woop\x07");

        assert_eq!(term.take_events(), [TerminalEvent::Title("woop".into())]);
        assert!(term.take_events().is_empty());
    }

    #[test]
    fn answers_cursor_position_queries() {
        let mut term = terminal(5, 10);
        term.advance(b"\x1b[3;4H\x1b[6n");

        assert_eq!(
            term.take_events(),
            [TerminalEvent::Reply("\x1b[3;4R".into())]
        );
    }

    #[test]
    fn resizes_screen() {
        let mut term = terminal(2, 10);
        term.resize(TerminalSize::new(4, 20));

        assert_eq!(term.size(), TerminalSize::new(4, 20));
        assert_eq!(term.screen_lines().len(), 4);
    }

    #[test]
    fn size_is_never_zero() {
        assert_eq!(TerminalSize::new(0, 0), TerminalSize::new(1, 1));
    }
}
