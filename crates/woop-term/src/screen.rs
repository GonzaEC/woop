//! A renderer-friendly snapshot of the visible screen.
//!
//! [`Screen`] copies the visible cells out of the emulator into plain Woop
//! types, so the interface can draw them without depending on the emulation
//! library.

use alacritty_terminal::term::RenderableContent;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{
    Color as AnsiColor, CursorShape as AnsiCursorShape, NamedColor,
};

/// Colour of a cell, before a theme is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// The theme's default foreground colour.
    Foreground,
    /// The theme's default background colour.
    Background,
    /// An entry of the 256-colour palette. Entries 0 to 15 are the theme's
    /// ANSI colours; the rest follow the standard 256-colour layout.
    Indexed(u8),
    /// An exact colour.
    Rgb(u8, u8, u8),
}

/// Text attributes of a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellStyle {
    /// Bold text.
    pub bold: bool,
    /// Faint text.
    pub dim: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
    /// Struck-through text.
    pub strikethrough: bool,
    /// Foreground and background are swapped.
    pub inverse: bool,
    /// Text is invisible.
    pub hidden: bool,
}

/// One character cell of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// Character in the cell. A space for empty cells.
    pub ch: char,
    /// Combining characters drawn on top of `ch`, usually empty.
    pub combining: Vec<char>,
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Text attributes.
    pub style: CellStyle,
    /// The character occupies this cell and the next one.
    pub wide: bool,
    /// This cell is the right half of a wide character and draws nothing.
    pub spacer: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            combining: Vec::new(),
            fg: Color::Foreground,
            bg: Color::Background,
            style: CellStyle::default(),
            wide: false,
            spacer: false,
        }
    }
}

/// Shape of the text cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// A filled block over the cell.
    Block,
    /// An outlined block, used when the window is not focused.
    HollowBlock,
    /// A line under the cell.
    Underline,
    /// A vertical bar at the left of the cell.
    Beam,
}

/// The text cursor, as visible on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Row, counted from the top of the visible screen.
    pub row: usize,
    /// Column, counted from the left edge.
    pub col: usize,
    /// Shape requested by the application.
    pub shape: CursorShape,
}

/// Snapshot of the visible screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// Number of visible rows.
    pub rows: usize,
    /// Number of visible columns.
    pub cols: usize,
    /// Cells in row-major order: `cells[row * cols + col]`.
    pub cells: Vec<Cell>,
    /// The cursor, or `None` when it is hidden or scrolled out of view.
    pub cursor: Option<Cursor>,
    /// How many lines the view is scrolled back into history.
    pub scroll_offset: usize,
}

impl Screen {
    /// Returns the cell at `row`, `col`.
    ///
    /// # Panics
    ///
    /// Panics if the position is outside the screen.
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row * self.cols + col]
    }

    /// Returns the cells of one row.
    ///
    /// # Panics
    ///
    /// Panics if `row` is outside the screen.
    pub fn row(&self, row: usize) -> &[Cell] {
        &self.cells[row * self.cols..(row + 1) * self.cols]
    }

    pub(crate) fn from_content(content: RenderableContent<'_>, rows: usize, cols: usize) -> Self {
        let mut cells = vec![Cell::default(); rows * cols];
        let offset = content.display_offset as i32;
        let colors = content.colors;
        let resolve = |color: AnsiColor| -> Color {
            match color {
                AnsiColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
                AnsiColor::Indexed(index) => match colors[usize::from(index)] {
                    Some(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
                    None => Color::Indexed(index),
                },
                AnsiColor::Named(named) => match colors[named as usize] {
                    Some(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
                    None => named_color(named),
                },
            }
        };

        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            let col = indexed.point.column.0;
            let (Ok(row), true) = (usize::try_from(row), col < cols) else {
                continue;
            };
            if row >= rows {
                continue;
            }
            let source = indexed.cell;
            let flags = source.flags;
            let cell = &mut cells[row * cols + col];
            cell.ch = if flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                source.c
            };
            if let Some(extra) = source.zerowidth() {
                cell.combining.extend_from_slice(extra);
            }
            cell.fg = resolve(source.fg);
            cell.bg = resolve(source.bg);
            cell.style = CellStyle {
                bold: flags.contains(Flags::BOLD),
                dim: flags.contains(Flags::DIM) || is_dim(source.fg),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikethrough: flags.contains(Flags::STRIKEOUT),
                inverse: flags.contains(Flags::INVERSE),
                hidden: flags.contains(Flags::HIDDEN),
            };
            cell.wide = flags.contains(Flags::WIDE_CHAR);
            cell.spacer =
                flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
        }

        let cursor = cursor_shape(content.cursor.shape).and_then(|shape| {
            let row = usize::try_from(content.cursor.point.line.0 + offset).ok()?;
            let col = content.cursor.point.column.0;
            (row < rows && col < cols).then_some(Cursor { row, col, shape })
        });

        Self {
            rows,
            cols,
            cells,
            cursor,
            scroll_offset: content.display_offset,
        }
    }
}

fn named_color(named: NamedColor) -> Color {
    let index = named as usize;
    if index < 16 {
        return Color::Indexed(index as u8);
    }
    let dim_black = NamedColor::DimBlack as usize;
    match named {
        NamedColor::Background => Color::Background,
        NamedColor::Foreground
        | NamedColor::BrightForeground
        | NamedColor::DimForeground
        | NamedColor::Cursor => Color::Foreground,
        _ if (dim_black..dim_black + 8).contains(&index) => {
            Color::Indexed((index - dim_black) as u8)
        }
        _ => Color::Foreground,
    }
}

fn is_dim(color: AnsiColor) -> bool {
    let AnsiColor::Named(named) = color else {
        return false;
    };
    let index = named as usize;
    let dim_black = NamedColor::DimBlack as usize;
    named == NamedColor::DimForeground || (dim_black..dim_black + 8).contains(&index)
}

fn cursor_shape(shape: AnsiCursorShape) -> Option<CursorShape> {
    match shape {
        AnsiCursorShape::Block => Some(CursorShape::Block),
        AnsiCursorShape::HollowBlock => Some(CursorShape::HollowBlock),
        AnsiCursorShape::Underline => Some(CursorShape::Underline),
        AnsiCursorShape::Beam => Some(CursorShape::Beam),
        AnsiCursorShape::Hidden => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Terminal, TerminalSize};

    fn screen_after(rows: u16, cols: u16, bytes: &[u8]) -> Screen {
        let mut term = Terminal::new(TerminalSize::new(rows, cols));
        term.advance(bytes);
        term.screen()
    }

    #[test]
    fn copies_characters_in_row_major_order() {
        let screen = screen_after(2, 4, b"ab\r\ncd");

        assert_eq!((screen.rows, screen.cols, screen.cells.len()), (2, 4, 8));
        assert_eq!(screen.cell(0, 1).ch, 'b');
        assert_eq!(screen.cell(1, 0).ch, 'c');
        assert_eq!(screen.row(1)[1].ch, 'd');
    }

    #[test]
    fn maps_ansi_and_true_colors() {
        let screen = screen_after(1, 10, b"\x1b[31ma\x1b[44mb\x1b[38;2;1;2;3mc\x1b[0md");

        assert_eq!(screen.cell(0, 0).fg, Color::Indexed(1));
        assert_eq!(screen.cell(0, 1).bg, Color::Indexed(4));
        assert_eq!(screen.cell(0, 2).fg, Color::Rgb(1, 2, 3));
        assert_eq!(screen.cell(0, 3).fg, Color::Foreground);
        assert_eq!(screen.cell(0, 3).bg, Color::Background);
    }

    #[test]
    fn maps_256_color_palette() {
        let screen = screen_after(1, 4, b"\x1b[38;5;208mx");

        assert_eq!(screen.cell(0, 0).fg, Color::Indexed(208));
    }

    #[test]
    fn maps_text_attributes() {
        let screen = screen_after(1, 10, b"\x1b[1ma\x1b[0;3;4mb\x1b[0;7mc\x1b[0;2md");

        assert!(screen.cell(0, 0).style.bold);
        assert!(screen.cell(0, 1).style.italic && screen.cell(0, 1).style.underline);
        assert!(screen.cell(0, 2).style.inverse);
        assert!(screen.cell(0, 3).style.dim);
    }

    #[test]
    fn marks_wide_characters_and_spacers() {
        let screen = screen_after(1, 4, "日".as_bytes());

        assert!(screen.cell(0, 0).wide);
        assert!(screen.cell(0, 1).spacer);
    }

    #[test]
    fn reports_cursor_and_hides_it_on_request() {
        let screen = screen_after(3, 10, b"\x1b[2;4H");
        assert_eq!(
            screen.cursor,
            Some(Cursor {
                row: 1,
                col: 3,
                shape: CursorShape::Block
            })
        );

        let hidden = screen_after(3, 10, b"\x1b[?25l");
        assert_eq!(hidden.cursor, None);
    }

    #[test]
    fn scrolling_shows_history() {
        let mut term = Terminal::new(TerminalSize::new(2, 10));
        term.advance(b"one\r\ntwo\r\nthree");
        assert_eq!(term.screen().cell(0, 0).ch, 't');

        term.scroll(1);
        let screen = term.screen();
        assert_eq!(screen.scroll_offset, 1);
        assert_eq!(screen.cell(0, 0).ch, 'o');
        assert_eq!(screen.cursor, None, "cursor scrolls out of view");

        term.scroll_to_bottom();
        assert_eq!(term.scroll_offset(), 0);
    }
}
