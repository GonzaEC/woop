//! Terminal state: escape sequence parsing and the screen grid.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Osc52, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Timeout};

use crate::blocks::{self, Block, BlockList, BlockRows};
use crate::marks::{Mark, MarkScanner};
use crate::screen::Screen;
use crate::selection::{SelectionKind, SelectionPoint};

/// Size of the terminal screen, in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    /// Number of visible rows.
    pub rows: u16,
    /// Number of visible columns.
    pub cols: u16,
}

/// Fewest columns a terminal can have: a wide character takes two, and the
/// emulator cannot place one in a narrower grid.
pub const MIN_COLUMNS: u16 = 2;

impl TerminalSize {
    /// Creates a size of `rows` by `cols` cells.
    ///
    /// Sizes are clamped to at least one row and [`MIN_COLUMNS`] columns.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.max(1),
            cols: cols.max(MIN_COLUMNS),
        }
    }

    /// Returns this size clamped like [`TerminalSize::new`], for sizes
    /// built directly from their fields.
    fn clamped(self) -> Self {
        Self::new(self.rows, self.cols)
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
            Event::Title(title) => TerminalEvent::Title(sanitize_title(&title)),
            Event::ResetTitle => TerminalEvent::ResetTitle,
            Event::Bell => TerminalEvent::Bell,
            Event::PtyWrite(text) => TerminalEvent::Reply(text),
            _ => return,
        };
        // The receiver lives as long as the terminal, so a failed send only
        // happens during teardown and can be ignored.
        let _ = self.sender.send(event);
    }
}

/// Removes control characters from a title set by a program and limits its
/// length: the title reaches the operating system's window decorations.
fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .filter(|&ch| !ch.is_control() && !is_invisible_format(ch))
        .take(MAX_TITLE_CHARS)
        .collect()
}

/// Invisible formatting characters that can make a title read differently
/// from what it contains: bidirectional overrides and isolates, zero-width
/// characters and the byte order mark.
fn is_invisible_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}'
    )
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
    marks: MarkScanner,
    blocks: BlockList,
    /// History size after the last update, to notice when it is cleared.
    history: usize,
    /// The window was resized while the alternate screen was shown, so the
    /// hidden scrollback was reflowed and must be recounted.
    reflowed_unseen: bool,
    /// The start of a character cut off at the end of the last output,
    /// waiting for the rest of it.
    utf8_carry: Vec<u8>,
}

/// Length of the incomplete UTF-8 character at the end of `bytes`: a lead
/// byte followed by fewer continuation bytes than it announces. Holding at
/// most three bytes back is always safe; invalid sequences are left to the
/// parser, which handles them when they are not split.
fn incomplete_utf8_suffix(bytes: &[u8]) -> usize {
    let tail = &bytes[bytes.len().saturating_sub(3)..];
    for (back, &byte) in tail.iter().rev().enumerate() {
        let needed = match byte {
            0x80..=0xbf => continue,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return 0,
        };
        let have = back + 1;
        return if have < needed { have } else { 0 };
    }
    0
}

/// Splits `bytes` into pieces of at most `size` bytes (more only if a single
/// character is longer), never cutting a UTF-8 character in two.
fn utf8_pieces(mut bytes: &[u8], size: usize) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        if bytes.is_empty() {
            return None;
        }
        let mut end = bytes.len().min(size);
        if end < bytes.len() {
            let cut = incomplete_utf8_suffix(&bytes[..end]);
            if cut < end {
                end -= cut;
            }
        }
        let (piece, rest) = bytes.split_at(end);
        bytes = rest;
        Some(piece)
    })
}

/// Lines of scrollback kept, at least.
pub(crate) const HISTORY_LINES: usize = 10_000;

/// Longest window title accepted from a program, in characters.
const MAX_TITLE_CHARS: usize = 256;

/// The history may grow this many lines past [`HISTORY_LINES`] before it is
/// trimmed back. Each trim also moves every block up, which costs a pass
/// over the block list, so trims are batched. The batch stays below the
/// emulator's cache of 1,000 spare rows: larger trims make it rotate and
/// reallocate the whole scrollback (measured: up to 68% slower output and
/// 30% more memory with a slack of 2,048).
pub(crate) const TRIM_SLACK: usize = 400;

/// Output is fed to the emulator in pieces of at most this many bytes, and
/// the history is checked after each one. Text adds at most one line per
/// byte, so with a limit of [`HISTORY_LINES`] + [`TRIM_SLACK`] + `FEED_CHUNK`
/// the emulator normally never drops history lines on its own and Woop knows
/// exactly how many it removed. Sequences that scroll many lines at once can
/// exceed that; the limit check in `update_history` then notices and forgets
/// the blocks.
const FEED_CHUNK: usize = 1024;

/// History limit given to the emulator.
const EMULATOR_HISTORY: usize = HISTORY_LINES + TRIM_SLACK + FEED_CHUNK;

impl Terminal {
    /// Creates an empty terminal of the given size.
    pub fn new(size: TerminalSize) -> Self {
        let size = size.clamped();
        let (sender, events) = mpsc::channel();
        let config = Config {
            scrolling_history: EMULATOR_HISTORY,
            // Programs may not read or write the clipboard (OSC 52): output
            // is untrusted, and a program that sets the clipboard decides
            // what the user pastes next. See ADR 0012.
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let term = Term::new(config, &size, EventProxy { sender });
        Self {
            term,
            parser: Processor::new(),
            events,
            size,
            marks: MarkScanner::default(),
            blocks: BlockList::default(),
            history: 0,
            reflowed_unseen: false,
            utf8_carry: Vec::new(),
        }
    }

    /// Processes output produced by the process running in the terminal.
    pub fn advance(&mut self, bytes: &[u8]) {
        if self.utf8_carry.is_empty() {
            self.advance_joined(bytes);
        } else {
            let mut joined = std::mem::take(&mut self.utf8_carry);
            joined.extend_from_slice(bytes);
            self.advance_joined(&joined);
        }
    }

    /// Processes output that starts on a character boundary. A character
    /// cut off at the end is kept for the next call: the emulator's parser
    /// can skip up to two bytes, an ESC among them, after a character split
    /// between two calls, which would put it out of step with the mark
    /// scanner and change what is shown.
    fn advance_joined(&mut self, bytes: &[u8]) {
        let (bytes, rest) = bytes.split_at(bytes.len() - incomplete_utf8_suffix(bytes));
        self.utf8_carry.extend_from_slice(rest);
        let now = Instant::now();
        let mut start = 0;
        for (end, mark) in self.marks.scan(bytes) {
            self.feed(&bytes[start..end]);
            let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
            if mark == Mark::SequenceTooLong {
                // CAN cancels the sequence in the emulator, which would
                // otherwise keep buffering it for as long as output goes on.
                self.parser.advance(&mut self.term, &[0x18]);
            } else if mark == Mark::HistoryCleared {
                self.history_cleared(alt_screen);
            } else if !alt_screen {
                // Full-screen applications do not emit marks, and the
                // alternate screen has no history for blocks to live in.
                self.blocks.apply(mark, &self.term, now);
            }
            start = end;
        }
        self.feed(&bytes[start..]);
    }

    /// Forgets blocks after the output cleared the scrollback. The clear is
    /// detected in the stream itself, so it is noticed even when more output
    /// in the same read refills the history.
    fn history_cleared(&mut self, alt_screen: bool) {
        self.blocks.clear();
        if alt_screen {
            // The main screen's history is hidden; recount it on return.
            self.reflowed_unseen = true;
        } else {
            self.blocks.recount_history(&self.term);
            self.history = self.term.grid().history_size();
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        for piece in utf8_pieces(bytes, FEED_CHUNK) {
            self.parser.advance(&mut self.term, piece);
            // Synchronized updates (mode 2026) make the parser hold output
            // back until the program ends them, which a hostile program never
            // does. Woop draws once per pump anyway, so any update is ended
            // right away: output, marks and clears are then applied in the
            // order they arrive.
            if self.parser.sync_timeout().pending_timeout() {
                self.parser.stop_sync(&mut self.term);
            }
            self.clamp_view();
            self.update_history(false);
        }
    }

    /// Keeps the view within the history. While the view is scrolled back,
    /// the emulator moves it along with new output but limits it by the
    /// history's capacity rather than its current size, so some output can
    /// push the view past the oldest line, and drawing would then read rows
    /// that do not exist.
    fn clamp_view(&mut self) {
        let grid = self.term.grid();
        let excess = grid.display_offset().saturating_sub(grid.history_size());
        if excess > 0 {
            let excess = i32::try_from(excess).unwrap_or(i32::MAX);
            self.term.scroll_display(Scroll::Delta(-excess));
        }
    }

    /// Trims the history back to [`HISTORY_LINES`] once it exceeds the slack,
    /// and keeps block positions in
    /// step with it.
    fn update_history(&mut self, resized: bool) {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            // The scrollback is hidden, but a resize still reflows it; catch
            // up once the alternate screen is closed.
            self.reflowed_unseen |= resized;
            return;
        }
        let reflowed = resized || std::mem::take(&mut self.reflowed_unseen);
        let history = self.term.grid().history_size();
        // The history only shrinks on its own when it is cleared (or when a
        // resize pulls lines back onto the screen), and it only reaches the
        // emulator's limit if lines may have been dropped without notice.
        // Either way, block positions can no longer be trusted.
        let lost = (!reflowed && history < self.history) || history >= EMULATOR_HISTORY;
        if lost {
            self.blocks.clear();
        }
        if reflowed || lost {
            self.blocks.recount_history(&self.term);
        } else if history > self.history {
            self.blocks.history_grew(&self.term, history - self.history);
        }
        if history > HISTORY_LINES + TRIM_SLACK {
            let excess = history - HISTORY_LINES;
            let removed = blocks::line_starts_in_top_rows(&self.term, excess);
            let grid = self.term.grid_mut();
            grid.update_history(HISTORY_LINES);
            grid.update_history(EMULATOR_HISTORY);
            self.blocks.remove_top_lines(removed);
        }
        self.history = self.term.grid().history_size();
    }

    /// Changes the screen size, reflowing content as needed.
    pub fn resize(&mut self, size: TerminalSize) {
        let size = size.clamped();
        if size != self.size {
            self.term.resize(size);
            self.size = size;
            self.update_history(true);
        }
    }

    /// Returns the command blocks, oldest first. Blocks are only produced
    /// when the shell runs Woop's shell integration.
    pub fn blocks(&self) -> &[Block] {
        self.blocks.as_slice()
    }

    /// Returns the rows `block` spans, or `None` while a full-screen
    /// application is shown or once the block left the scrollback.
    pub fn block_rows(&self, block: &Block) -> Option<BlockRows> {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        self.blocks.rows(&self.term, block)
    }

    /// Number of lines in the scrollback above the screen. Together with
    /// [`Terminal::scroll_offset`] it converts [`BlockRows`] into screen
    /// rows: `screen_row = row - (history_size - scroll_offset)`.
    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
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

    /// Starts a new selection at `point`, replacing any previous one.
    pub fn start_selection(&mut self, kind: SelectionKind, point: SelectionPoint) {
        let kind = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        let (point, side) = self.grid_point(point);
        self.term.selection = Some(Selection::new(kind, point, side));
    }

    /// Moves the end of the current selection to `point`. Does nothing if
    /// there is no selection.
    pub fn extend_selection(&mut self, point: SelectionPoint) {
        let (point, side) = self.grid_point(point);
        if let Some(selection) = &mut self.term.selection {
            selection.update(point, side);
        }
    }

    /// Removes the current selection.
    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// Returns whether any text is selected.
    pub fn has_selection(&self) -> bool {
        self.term
            .selection
            .as_ref()
            .is_some_and(|selection| !selection.is_empty())
    }

    /// Returns the selected text, or `None` if nothing is selected.
    ///
    /// Rows joined by wrapping are copied as one line, and trailing
    /// whitespace at the end of each line is removed.
    pub fn selection_text(&self) -> Option<String> {
        if !self.has_selection() {
            return None;
        }
        self.term
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    /// Converts a point on the visible screen into a grid point, taking the
    /// scroll position into account.
    fn grid_point(&self, point: SelectionPoint) -> (Point, Side) {
        let grid = self.term.grid();
        let row = point.row.min(grid.screen_lines() - 1) as i32;
        let col = point.col.min(grid.columns() - 1);
        let line = Line(row - grid.display_offset() as i32);
        let side = if point.right_half {
            Side::Right
        } else {
            Side::Left
        };
        (Point::new(line, Column(col)), side)
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
    fn an_endless_sequence_is_cut_off() {
        // The parser buffers OSC payloads without limit; an unterminated
        // one would grow memory for as long as the output continues. A
        // control character between ESC and `]` must not hide it.
        for start in [
            "\x1b]0;",
            "\x1b\x00]0;",
            "\x1b\r]0;",
            "\x1b\x7f]0;",
            "\x1b\u{e9}]0;",
            "\x1b]0;t\x1b\x7f]0;",
        ] {
            let mut term = terminal(3, 20);
            let endless = format!("{start}{}", "x".repeat(200 * 1024));
            term.advance(endless.as_bytes());
            term.advance(b"\r\nvisible");

            assert!(term.screen_text().contains("visible"), "{start:?}");
        }
    }

    #[test]
    fn output_is_never_held_back() {
        // A synchronized update that never ends would hide all further
        // output, including what the user types.
        let mut term = terminal(3, 20);
        term.advance(b"\x1b[?2026hvisible");

        assert!(term.screen_text().contains("visible"));
    }

    #[test]
    fn window_titles_are_bounded_and_printable() {
        let mut term = terminal(2, 10);
        let long = "t".repeat(10_000);
        term.advance(format!("\x1b]0;a\x7fb\u{9b}\u{202e}\u{200b}c{long}\x07").as_bytes());

        let events = term.take_events();
        let [TerminalEvent::Title(title)] = events.as_slice() else {
            panic!("expected one title, got {events:?}");
        };
        assert!(title.starts_with("abc"));
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
        assert!(!title.chars().any(char::is_control));
    }

    #[test]
    fn programs_cannot_use_the_clipboard() {
        // OSC 52 would let any output (a file being displayed, a remote
        // host) replace what the user pastes next, or read it.
        let mut term = terminal(2, 10);
        term.advance(b"\x1b]52;c;cm0gLXJmIH4K\x07\x1b]52;c;?\x07");

        assert_eq!(term.take_events(), []);
    }

    #[test]
    fn resizes_screen() {
        let mut term = terminal(2, 10);
        term.resize(TerminalSize::new(4, 20));

        assert_eq!(term.size(), TerminalSize::new(4, 20));
        assert_eq!(term.screen_lines().len(), 4);
    }

    fn right(row: usize, col: usize) -> SelectionPoint {
        SelectionPoint {
            right_half: true,
            ..SelectionPoint::new(row, col)
        }
    }

    #[test]
    fn selects_text_across_rows() {
        let mut term = terminal(3, 20);
        term.advance(b"hello world\r\nsecond line");

        term.start_selection(SelectionKind::Simple, SelectionPoint::new(0, 6));
        term.extend_selection(right(1, 5));

        assert_eq!(term.selection_text().as_deref(), Some("world\nsecond"));
    }

    #[test]
    fn a_click_without_drag_selects_nothing() {
        let mut term = terminal(2, 20);
        term.advance(b"hello");

        term.start_selection(SelectionKind::Simple, SelectionPoint::new(0, 2));
        assert_eq!(term.selection_text(), None);

        term.start_selection(SelectionKind::Simple, SelectionPoint::new(0, 0));
        term.extend_selection(right(0, 1));
        term.clear_selection();
        assert_eq!(term.selection_text(), None);
    }

    #[test]
    fn selects_words_and_lines() {
        let mut term = terminal(2, 30);
        term.advance(b"cargo build --release\r\nnext");

        term.start_selection(SelectionKind::Word, SelectionPoint::new(0, 7));
        assert_eq!(term.selection_text().as_deref(), Some("build"));

        term.start_selection(SelectionKind::Line, SelectionPoint::new(0, 3));
        assert_eq!(
            term.selection_text().as_deref(),
            Some("cargo build --release\n")
        );
    }

    #[test]
    fn selection_follows_the_scroll_position() {
        let mut term = terminal(2, 10);
        term.advance(b"one\r\ntwo\r\nthree");
        term.scroll(1);

        term.start_selection(SelectionKind::Line, SelectionPoint::new(0, 0));
        assert_eq!(term.selection_text().as_deref(), Some("one\n"));
    }

    /// The emulator's parser can skip up to two bytes (including an ESC)
    /// after a character split between two reads. Woop never splits one.
    #[test]
    fn characters_split_between_reads_are_joined() {
        for (first, second) in [
            (&b"e\xcc"[..], &b"\x81\x1b\x800"[..]),
            (&b"\xce"[..], &b"\x99)\xb5"[..]),
            (&b"x\xe6\x97"[..], &b"\xa5\x1b\xe6\x97\xa5"[..]),
        ] {
            let mut whole = terminal(2, 20);
            whole.advance(&[first, second].concat());
            let mut split = terminal(2, 20);
            split.advance(first);
            split.advance(second);

            assert_eq!(split.screen(), whole.screen(), "{first:?} | {second:?}");
        }
    }

    #[test]
    fn utf8_suffixes() {
        assert_eq!(incomplete_utf8_suffix(b"abc"), 0);
        assert_eq!(incomplete_utf8_suffix(b"a\xe6\x97"), 2);
        assert_eq!(incomplete_utf8_suffix(b"a\xe6\x97\xa5"), 0);
        assert_eq!(incomplete_utf8_suffix(b"\xf0\x9f\x98"), 3);
        assert_eq!(incomplete_utf8_suffix(b"\xcc"), 1);
        assert_eq!(incomplete_utf8_suffix(b"\x80\x80"), 0);
        assert_eq!(incomplete_utf8_suffix(b""), 0);
    }

    /// Output arriving while the view is scrolled back could leave the
    /// emulator's view offset larger than its history, so drawing read rows
    /// that do not exist (a panic in debug builds, wrong rows in release).
    #[test]
    fn the_view_never_scrolls_past_the_history() {
        let mut term = terminal(3, 2);
        term.advance(b"\x1b[2;4r\x1b[2J");
        term.scroll(1);
        term.advance("日本   ".as_bytes());

        assert!(term.scroll_offset() <= term.history_size());
        let _ = term.screen();
    }

    /// A wide character needs two columns; with one, the emulator panicked.
    /// A window shrunk to its minimum produced exactly that grid.
    #[test]
    fn wide_characters_in_the_narrowest_terminal() {
        let narrow = TerminalSize { rows: 1, cols: 1 };
        let mut term = Terminal::new(narrow);
        term.advance("日本".as_bytes());

        let mut term = terminal(3, 10);
        term.resize(narrow);
        term.advance("日本".as_bytes());
        assert_eq!(term.size().cols, MIN_COLUMNS);
    }

    #[test]
    fn size_is_never_zero() {
        assert_eq!(TerminalSize::new(0, 0), TerminalSize { rows: 1, cols: 2 });
    }
}
