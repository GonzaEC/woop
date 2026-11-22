//! Command blocks: each command, its output and its result.
//!
//! Blocks are built from the shell integration marks (see `marks`). Their
//! positions are stored as *logical lines*: a logical line is a row together
//! with the rows it wraps into. Counting logical lines from the top of the
//! scrollback, instead of rows, keeps positions valid when the window is
//! resized, because reflowing text changes how many rows a line takes but
//! not how many lines there are.

use std::time::{Duration, Instant};

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;

use crate::marks::Mark;

/// Maximum number of blocks kept. Older blocks are forgotten first.
const MAX_BLOCKS: usize = 10_000;

/// Where a block is in its life cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    /// The prompt is shown and the user is typing a command.
    Prompt,
    /// The command is running.
    Running,
    /// The command finished.
    Finished,
}

/// One command and its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    prompt: Anchor,
    input: Option<Anchor>,
    output: Option<Anchor>,
    end: Option<Anchor>,
    command: String,
    exit_code: Option<i32>,
    started_at: Option<Instant>,
    duration: Option<Duration>,
}

impl Block {
    fn new(prompt: Anchor) -> Self {
        Self {
            prompt,
            input: None,
            output: None,
            end: None,
            command: String::new(),
            exit_code: None,
            started_at: None,
            duration: None,
        }
    }

    /// Returns where the block is in its life cycle.
    pub fn state(&self) -> BlockState {
        match (self.output, self.end) {
            (_, Some(_)) => BlockState::Finished,
            (Some(_), None) => BlockState::Running,
            (None, None) => BlockState::Prompt,
        }
    }

    /// The command line as the user typed it. Empty until the command is
    /// submitted.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Exit code of the command, once it finished, if the shell reported one.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// When the command was submitted.
    pub fn started_at(&self) -> Option<Instant> {
        self.started_at
    }

    /// How long the command ran, once it finished.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

/// Rows a block spans, counted from the top of the scrollback: row 0 is the
/// oldest line kept in history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRows {
    /// First row of the prompt.
    pub prompt: usize,
    /// First row of the output, once the command was submitted.
    pub output: Option<usize>,
    /// Row where the command finished, which is usually where the next
    /// prompt starts.
    pub end: Option<usize>,
}

/// A position in the scrollback that survives reflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor {
    /// Logical line, counted from the top of the scrollback.
    line: usize,
    /// Cell offset within the logical line.
    offset: usize,
}

/// The blocks of a terminal, oldest first.
#[derive(Debug, Default)]
pub(crate) struct BlockList {
    blocks: Vec<Block>,
}

impl BlockList {
    pub(crate) fn as_slice(&self) -> &[Block] {
        &self.blocks
    }

    /// Updates the blocks for a mark received with the cursor at its
    /// current position.
    pub(crate) fn apply<T>(&mut self, mark: Mark, term: &Term<T>, now: Instant) {
        let anchor = anchor_at(term, term.grid().cursor.point);
        match mark {
            Mark::PromptStart => {
                // A prompt that never ran a command (for example after
                // Ctrl+C at the prompt) is replaced.
                if self
                    .blocks
                    .last()
                    .is_some_and(|block| block.state() == BlockState::Prompt)
                {
                    self.blocks.pop();
                }
                // A running command that never reported its end is closed.
                if let Some(block) = self.current(BlockState::Running) {
                    block.end = Some(anchor);
                }
                self.blocks.push(Block::new(anchor));
                if self.blocks.len() > MAX_BLOCKS {
                    self.blocks.remove(0);
                }
            }
            Mark::CommandStart => {
                if let Some(block) = self.current(BlockState::Prompt) {
                    block.input = Some(anchor);
                }
            }
            Mark::OutputStart => {
                if let Some(block) = self.current(BlockState::Prompt) {
                    let start = block.input.unwrap_or(block.prompt);
                    block.command = text_between(term, start, anchor);
                    block.output = Some(anchor);
                    block.started_at = Some(now);
                }
            }
            Mark::CommandFinished(code) => {
                let Some(block) = self.current(BlockState::Running) else {
                    return;
                };
                if block.command.is_empty() {
                    // Submitting an empty line is not a command.
                    self.blocks.pop();
                    return;
                }
                block.end = Some(anchor);
                block.exit_code = code;
                block.duration = block.started_at.map(|start| now - start);
            }
        }
    }

    /// Accounts for `lines` logical lines removed from the top of the
    /// scrollback. Blocks whose prompt was removed are forgotten.
    pub(crate) fn remove_top_lines(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        self.blocks.retain(|block| block.prompt.line >= lines);
        for block in &mut self.blocks {
            for anchor in [
                Some(&mut block.prompt),
                block.input.as_mut(),
                block.output.as_mut(),
                block.end.as_mut(),
            ]
            .into_iter()
            .flatten()
            {
                anchor.line -= lines;
            }
        }
    }

    /// Forgets every block, for when positions can no longer be trusted.
    pub(crate) fn clear(&mut self) {
        self.blocks.clear();
    }

    /// Returns the rows of `block` in the current grid.
    pub(crate) fn rows<T>(term: &Term<T>, block: &Block) -> Option<BlockRows> {
        let history = term.grid().history_size();
        let absolute = |point: Point| (point.line.0 + history as i32) as usize;
        let row = |anchor: Option<Anchor>| anchor.and_then(|a| point_of(term, a)).map(absolute);
        Some(BlockRows {
            prompt: row(Some(block.prompt))?,
            output: row(block.output),
            end: row(block.end),
        })
    }

    fn current(&mut self, state: BlockState) -> Option<&mut Block> {
        self.blocks
            .last_mut()
            .filter(|block| block.state() == state)
    }
}

/// Counts the logical lines that start in the `rows` topmost rows of the
/// scrollback.
pub(crate) fn line_starts_in_top_rows<T>(term: &Term<T>, rows: usize) -> usize {
    let top = term.grid().topmost_line().0;
    (top..top + rows as i32)
        .filter(|&row| !wraps(term, Line(row)))
        .count()
}

/// Whether `row` continues on the next row.
fn wraps<T>(term: &Term<T>, row: Line) -> bool {
    let grid = term.grid();
    grid[row][Column(grid.columns() - 1)]
        .flags
        .contains(Flags::WRAPLINE)
}

/// Converts a grid point into an anchor.
fn anchor_at<T>(term: &Term<T>, point: Point) -> Anchor {
    let top = term.grid().topmost_line().0;
    let columns = term.grid().columns();
    let mut line = 0;
    let mut rows_into_line = 0;
    for row in top..point.line.0 {
        if wraps(term, Line(row)) {
            rows_into_line += 1;
        } else {
            line += 1;
            rows_into_line = 0;
        }
    }
    Anchor {
        line,
        offset: rows_into_line * columns + point.column.0,
    }
}

/// Converts an anchor back into a grid point, if it is still in the grid.
fn point_of<T>(term: &Term<T>, anchor: Anchor) -> Option<Point> {
    let grid = term.grid();
    let columns = grid.columns();
    let bottom = grid.bottommost_line().0;
    let mut line = 0;
    let mut row = grid.topmost_line().0;
    while line < anchor.line {
        if row > bottom {
            return None;
        }
        if !wraps(term, Line(row)) {
            line += 1;
        }
        row += 1;
    }
    // Walk down the wrapped rows of the logical line to the offset.
    let mut offset = anchor.offset;
    while offset >= columns && row < bottom && wraps(term, Line(row)) {
        offset -= columns;
        row += 1;
    }
    (row <= bottom).then(|| Point::new(Line(row), Column(offset.min(columns - 1))))
}

/// Returns the text between two anchors, with trailing whitespace removed.
fn text_between<T>(term: &Term<T>, start: Anchor, end: Anchor) -> String {
    let (Some(start), Some(end)) = (point_of(term, start), point_of(term, end)) else {
        return String::new();
    };
    if end <= start {
        return String::new();
    }
    // `bounds_to_string` includes its end point, so stop one cell earlier.
    let columns = term.grid().columns();
    let last = if end.column.0 > 0 {
        Point::new(end.line, end.column - 1)
    } else {
        Point::new(end.line - 1, Column(columns - 1))
    };
    term.bounds_to_string(start, last).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Terminal, TerminalSize};

    const A: &str = "\x1b]133;A\x07";
    const B: &str = "\x1b]133;B\x07";
    const C: &str = "\x1b]133;C\x07";

    fn d(code: i32) -> String {
        format!("\x1b]133;D;{code}\x07")
    }

    /// Output of a shell running `command`, which prints `output`, exits
    /// with `code` and shows the next prompt.
    fn run(command: &str, output: &str, code: i32) -> String {
        format!("{A}$ {B}{command}\r\n{C}{output}{}{A}$ {B}", d(code))
    }

    fn terminal(rows: u16, cols: u16, output: &str) -> Terminal {
        let mut term = Terminal::new(TerminalSize::new(rows, cols));
        term.advance(output.as_bytes());
        term
    }

    #[test]
    fn tracks_a_command_and_its_output() {
        let term = terminal(10, 40, &run("ls -la", "one\r\ntwo\r\n", 0));
        let blocks = term.blocks();

        assert_eq!(blocks.len(), 2);
        let block = &blocks[0];
        assert_eq!(block.state(), BlockState::Finished);
        assert_eq!(block.command(), "ls -la");
        assert_eq!(block.exit_code(), Some(0));
        assert!(block.duration().is_some());
        assert_eq!(
            term.block_rows(block),
            Some(BlockRows {
                prompt: 0,
                output: Some(1),
                end: Some(3)
            })
        );
        assert_eq!(blocks[1].state(), BlockState::Prompt);
    }

    #[test]
    fn reports_running_and_failed_commands() {
        let mut term = terminal(10, 40, &format!("{A}$ {B}make\r\n{C}building\r\n"));
        assert_eq!(term.blocks()[0].state(), BlockState::Running);
        assert_eq!(term.blocks()[0].duration(), None);

        term.advance(format!("{}{A}$ {B}", d(2)).as_bytes());
        assert_eq!(term.blocks()[0].exit_code(), Some(2));

        term.advance(format!("x\r\n{C}\x1b]133;D\x07{A}$ {B}").as_bytes());
        assert_eq!(term.blocks()[1].exit_code(), None);
        assert_eq!(term.blocks()[1].state(), BlockState::Finished);
    }

    #[test]
    fn empty_commands_and_abandoned_prompts_are_not_blocks() {
        let empty = run("", "", 0);
        let interrupted = format!("{A}$ {B}half typed^C\r\n{A}$ {B}");
        for output in [empty, interrupted] {
            let term = terminal(10, 40, &output);
            assert_eq!(term.blocks().len(), 1, "{output:?}");
            assert_eq!(term.blocks()[0].state(), BlockState::Prompt);
        }
    }

    #[test]
    fn positions_survive_reflow() {
        let long = "x".repeat(25);
        let mut term = terminal(6, 10, &run("cmd", &format!("{long}\r\n"), 0));
        let rows = term.block_rows(&term.blocks()[1]).expect("rows");
        assert_eq!(rows.prompt, 4, "25 characters take three rows of 10");

        term.resize(TerminalSize::new(6, 30));
        let rows = term.block_rows(&term.blocks()[1]).expect("rows");
        assert_eq!(rows.prompt, 2, "and one row of 30");
        assert_eq!(term.screen_lines()[2], "$");
    }

    #[test]
    fn follows_history_trimming() {
        let flood = "line\r\n".repeat(crate::terminal::HISTORY_LINES + 500);
        let mut output = run("flood", &flood, 0);
        output.push_str(&format!("last\r\n{C}done\r\n{}{A}$ {B}", d(0)));
        let term = terminal(5, 20, &output);

        // The first block's prompt left the history; the second remains.
        let blocks = term.blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].command(), "last");
        let rows = term.block_rows(&blocks[0]).expect("rows");
        let screen_row = rows.prompt - term.history_size();
        assert_eq!(term.screen_lines()[screen_row], "$ last");
    }

    #[test]
    fn clearing_the_history_forgets_blocks() {
        let mut term = terminal(3, 20, &run("ls", "a\r\nb\r\nc\r\nd\r\n", 0));
        assert_eq!(term.blocks().len(), 2);

        term.advance(b"\x1b[H\x1b[2J\x1b[3J");
        assert!(term.blocks().is_empty());
    }

    #[test]
    fn ignores_marks_on_the_alternate_screen() {
        let term = terminal(5, 20, &format!("\x1b[?1049h{A}$ {B}"));
        assert!(term.blocks().is_empty());
    }
}
