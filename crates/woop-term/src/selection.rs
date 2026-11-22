//! Types describing a text selection on the visible screen.

/// How a selection grows from the point where it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Character by character, wrapping across rows.
    Simple,
    /// Whole words, as selected with a double click.
    Word,
    /// Whole lines, as selected with a triple click.
    Line,
    /// A rectangle of cells.
    Block,
}

/// A position on the visible screen used to start or extend a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPoint {
    /// Row, counted from the top of the visible screen.
    pub row: usize,
    /// Column, counted from the left edge.
    pub col: usize,
    /// The point is on the right half of the cell. A selection only
    /// includes a cell once the pointer has passed its middle.
    pub right_half: bool,
}

impl SelectionPoint {
    /// Creates a point on the left half of the cell at `row`, `col`.
    pub fn new(row: usize, col: usize) -> Self {
        Self {
            row,
            col,
            right_half: false,
        }
    }
}
