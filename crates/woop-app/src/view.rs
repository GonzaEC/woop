//! The terminal widget: draws a [`Screen`] and turns window events into
//! messages.

use iced::advanced::graphics::core::alignment::Vertical;
use iced::advanced::text::{self, Paragraph as _};
use iced::mouse::{self, ScrollDelta};
use iced::widget::canvas::{self, Action, Frame, Geometry};
use std::time::{Duration, Instant};

use iced::{Color, Event, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, keyboard};
use woop_term::{Cell, CursorShape, Screen, SelectionKind, SelectionPoint, TerminalSize};

use crate::Message;
use crate::keys;
use crate::theme::{self, Palette};

/// Space between the window edge and the grid, in logical pixels.
const PADDING: f32 = 6.0;
/// Lines scrolled per mouse wheel notch.
const LINES_PER_NOTCH: f32 = 3.0;
/// Maximum time between clicks for them to count as a double or triple click.
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(400);

type Paragraph = <Renderer as text::Renderer>::Paragraph;

/// Size of one character cell, derived from the font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Font used for the grid.
    pub font: Font,
    /// Font size in logical pixels.
    pub font_size: f32,
    /// Width of one cell.
    pub cell_width: f32,
    /// Height of one cell.
    pub cell_height: f32,
}

impl Metrics {
    /// Measures the cell size of `font` at `font_size`.
    pub fn measure(font: Font, font_size: f32) -> Self {
        const SAMPLE: &str = "MMMMMMMMMMMMMMMMMMMM";
        let paragraph = Paragraph::with_text(text::Text {
            content: SAMPLE,
            bounds: Size::INFINITE,
            size: Pixels(font_size),
            line_height: text::LineHeight::Relative(1.25),
            font,
            align_x: text::Alignment::Left,
            align_y: Vertical::Top,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        });
        let bounds = paragraph.min_bounds();
        let samples = SAMPLE.chars().count() as f32;
        Self {
            font,
            font_size,
            cell_width: (bounds.width / samples).max(1.0),
            cell_height: bounds.height.ceil().max(1.0),
        }
    }

    /// Number of rows and columns that fit in `size`.
    pub fn grid_size(&self, size: Size) -> TerminalSize {
        let fit = |space: f32, cell: f32| {
            ((space - 2.0 * PADDING) / cell)
                .floor()
                .clamp(1.0, f32::from(u16::MAX))
        };
        TerminalSize::new(
            fit(size.height, self.cell_height) as u16,
            fit(size.width, self.cell_width) as u16,
        )
    }
}

/// Font for the terminal grid on the current platform.
pub fn default_font() -> Font {
    if cfg!(windows) {
        Font::with_name("Consolas")
    } else if cfg!(target_os = "macos") {
        Font::with_name("Menlo")
    } else {
        Font::MONOSPACE
    }
}

/// Canvas program that renders the terminal.
pub struct TerminalView<'a> {
    /// Screen to draw.
    pub screen: &'a Screen,
    /// Colours.
    pub palette: &'a Palette,
    /// Cell size.
    pub metrics: Metrics,
    /// What key presses mean right now.
    pub keys: keys::KeyContext,
    /// Cached geometry, cleared whenever the screen changes.
    pub cache: &'a canvas::Cache,
}

/// Widget state kept by iced between frames.
#[derive(Debug, Default)]
pub struct ViewState {
    size: Option<TerminalSize>,
    scroll_remainder: f32,
    /// The left button is held down and moves extend the selection.
    selecting: bool,
    /// Last point the selection was extended to, to skip repeated moves.
    drag_point: Option<SelectionPoint>,
    /// Time and cell of the last click, to detect double and triple clicks.
    last_click: Option<(Instant, SelectionPoint)>,
    /// Number of consecutive clicks on the same cell, from 1 to 3.
    clicks: u8,
    /// Keyboard modifiers currently held, for Alt+drag block selection.
    modifiers: keyboard::Modifiers,
}

impl ViewState {
    /// Registers a click and returns how many clicks in a row it makes.
    fn click(&mut self, point: SelectionPoint) -> u8 {
        let now = Instant::now();
        let repeated = self.last_click.is_some_and(|(time, last)| {
            now.duration_since(time) <= MULTI_CLICK_INTERVAL
                && (last.row, last.col) == (point.row, point.col)
        });
        self.clicks = if repeated { self.clicks % 3 + 1 } else { 1 };
        self.last_click = Some((now, point));
        self.clicks
    }
}

impl canvas::Program<Message> for TerminalView<'_> {
    type State = ViewState;

    fn update(
        &self,
        state: &mut ViewState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
        }
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => keys::key_press(key, *modifiers, text.as_deref(), self.keys)
                .map(|action| Action::publish(Message::Key(action)).and_capture()),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                let point = self.selection_point(position);
                let kind = match state.click(point) {
                    1 if state.modifiers.alt() => SelectionKind::Block,
                    1 => SelectionKind::Simple,
                    2 => SelectionKind::Word,
                    _ => SelectionKind::Line,
                };
                state.selecting = true;
                state.drag_point = Some(point);
                Some(Action::publish(Message::SelectionStarted(point, kind)).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.selecting => {
                let point =
                    self.selection_point(Point::new(position.x - bounds.x, position.y - bounds.y));
                (state.drag_point != Some(point)).then(|| {
                    state.drag_point = Some(point);
                    Action::publish(Message::SelectionExtended(point)).and_capture()
                })
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.selecting => {
                state.selecting = false;
                state.drag_point = None;
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    ScrollDelta::Lines { y, .. } => y * LINES_PER_NOTCH,
                    ScrollDelta::Pixels { y, .. } => y / self.metrics.cell_height,
                } + state.scroll_remainder;
                let whole = lines.trunc();
                state.scroll_remainder = lines - whole;
                (whole != 0.0).then(|| Action::publish(Message::Scroll(whole as i32)).and_capture())
            }
            _ => {
                let size = self.metrics.grid_size(bounds.size());
                (state.size != Some(size)).then(|| {
                    state.size = Some(size);
                    Action::publish(Message::Resized(size))
                })
            }
        }
    }

    fn mouse_interaction(
        &self,
        state: &ViewState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.selecting || cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &ViewState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            frame.fill_rectangle(Point::ORIGIN, frame.size(), self.palette.background);
            for row in 0..self.screen.rows {
                self.draw_row(frame, row);
            }
            self.draw_cursor(frame);
        });
        vec![geometry]
    }
}

/// Resolved colours and font of a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    fg: Color,
    bg: Color,
    font: Font,
}

impl TerminalView<'_> {
    fn origin(&self, row: usize, col: usize) -> Point {
        Point::new(
            PADDING + col as f32 * self.metrics.cell_width,
            PADDING + row as f32 * self.metrics.cell_height,
        )
    }

    /// Converts a position relative to the widget into the cell under it.
    /// Positions outside the grid are clamped to its edges, so dragging past
    /// them selects up to the first or last cell.
    fn selection_point(&self, position: Point) -> SelectionPoint {
        let metrics = self.metrics;
        let last_row = self.screen.rows.saturating_sub(1);
        let last_col = self.screen.cols.saturating_sub(1);
        let x = ((position.x - PADDING) / metrics.cell_width).max(0.0);
        let y = ((position.y - PADDING) / metrics.cell_height).max(0.0);
        let col = x.floor() as usize;
        SelectionPoint {
            row: (y.floor() as usize).min(last_row),
            col: col.min(last_col),
            right_half: col > last_col || x.fract() >= 0.5,
        }
    }

    fn look(&self, cell: &Cell) -> Look {
        let style = cell.style;
        let mut fg = self.palette.resolve(cell.fg, style.bold);
        let mut bg = self.palette.resolve(cell.bg, false);
        if style.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.selected {
            bg = self.palette.selection;
        }
        if style.dim {
            fg = theme::dim(fg, bg);
        }
        if style.hidden {
            fg = bg;
        }
        let mut font = self.metrics.font;
        if style.bold {
            font.weight = iced::font::Weight::Bold;
        }
        if style.italic {
            font.style = iced::font::Style::Italic;
        }
        Look { fg, bg, font }
    }

    fn draw_row(&self, frame: &mut Frame, row: usize) {
        let cells = self.screen.row(row);
        let metrics = self.metrics;

        // Backgrounds: one rectangle per run of equal, non-default colour.
        let mut col = 0;
        while col < cells.len() {
            let bg = self.look(&cells[col]).bg;
            let start = col;
            while col < cells.len() && self.look(&cells[col]).bg == bg {
                col += 1;
            }
            if bg != self.palette.background {
                frame.fill_rectangle(
                    self.origin(row, start),
                    Size::new(
                        (col - start) as f32 * metrics.cell_width,
                        metrics.cell_height,
                    ),
                    bg,
                );
            }
        }

        // Text: one draw call per run of cells sharing colour and font.
        // Wide characters are drawn on their own so they cannot shift the
        // characters after them.
        let mut run = String::new();
        let mut run_start = 0;
        let mut run_look: Option<Look> = None;
        for (col, cell) in cells.iter().enumerate() {
            if cell.spacer {
                continue;
            }
            let look = self.look(cell);
            let breaks_run = cell.wide
                || run_look
                    .is_some_and(|current| current.fg != look.fg || current.font != look.font);
            if breaks_run {
                self.flush_text(frame, row, run_start, &mut run, run_look);
            }
            if run.is_empty() {
                run_start = col;
                run_look = Some(look);
            }
            run.push(cell.ch);
            run.extend(&cell.combining);
            if cell.wide {
                self.flush_text(frame, row, run_start, &mut run, run_look);
            }
            self.draw_decorations(frame, row, col, cell, look);
        }
        self.flush_text(frame, row, run_start, &mut run, run_look);
    }

    fn flush_text(
        &self,
        frame: &mut Frame,
        row: usize,
        col: usize,
        run: &mut String,
        look: Option<Look>,
    ) {
        if let Some(look) = look
            && !run.trim().is_empty()
        {
            frame.fill_text(canvas::Text {
                content: run.clone(),
                position: self.origin(row, col),
                color: look.fg,
                size: Pixels(self.metrics.font_size),
                line_height: text::LineHeight::Absolute(Pixels(self.metrics.cell_height)),
                font: look.font,
                align_y: Vertical::Top,
                shaping: text::Shaping::Basic,
                ..canvas::Text::default()
            });
        }
        run.clear();
    }

    fn draw_decorations(&self, frame: &mut Frame, row: usize, col: usize, cell: &Cell, look: Look) {
        let origin = self.origin(row, col);
        let width = self.metrics.cell_width * if cell.wide { 2.0 } else { 1.0 };
        let line = (self.metrics.font_size / 14.0).max(1.0);
        if cell.style.underline {
            let y = origin.y + self.metrics.cell_height - line * 2.0;
            frame.fill_rectangle(Point::new(origin.x, y), Size::new(width, line), look.fg);
        }
        if cell.style.strikethrough {
            let y = origin.y + self.metrics.cell_height / 2.0;
            frame.fill_rectangle(Point::new(origin.x, y), Size::new(width, line), look.fg);
        }
    }

    fn draw_cursor(&self, frame: &mut Frame) {
        let Some(cursor) = self.screen.cursor else {
            return;
        };
        let cell = self.screen.cell(cursor.row, cursor.col);
        let origin = self.origin(cursor.row, cursor.col);
        let width = self.metrics.cell_width * if cell.wide { 2.0 } else { 1.0 };
        let height = self.metrics.cell_height;
        let thickness = (self.metrics.font_size / 7.0).max(2.0).round();
        let color = self.palette.cursor;

        match cursor.shape {
            CursorShape::Block => {
                frame.fill_rectangle(origin, Size::new(width, height), color);
                if !cell.ch.is_whitespace() {
                    let mut run = cell.ch.to_string();
                    let look = Look {
                        fg: self.palette.background,
                        ..self.look(cell)
                    };
                    self.flush_text(frame, cursor.row, cursor.col, &mut run, Some(look));
                }
            }
            CursorShape::HollowBlock => {
                let edges = [
                    (origin, Size::new(width, 1.0)),
                    (
                        Point::new(origin.x, origin.y + height - 1.0),
                        Size::new(width, 1.0),
                    ),
                    (origin, Size::new(1.0, height)),
                    (
                        Point::new(origin.x + width - 1.0, origin.y),
                        Size::new(1.0, height),
                    ),
                ];
                for (position, size) in edges {
                    frame.fill_rectangle(position, size, color);
                }
            }
            CursorShape::Underline => frame.fill_rectangle(
                Point::new(origin.x, origin.y + height - thickness),
                Size::new(width, thickness),
                color,
            ),
            CursorShape::Beam => frame.fill_rectangle(origin, Size::new(thickness, height), color),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            font: Font::MONOSPACE,
            font_size: 14.0,
            cell_width: 8.0,
            cell_height: 18.0,
        }
    }

    #[test]
    fn grid_size_accounts_for_padding() {
        let size = metrics().grid_size(Size::new(
            8.0 * 80.0 + 2.0 * PADDING,
            18.0 * 24.0 + 2.0 * PADDING,
        ));
        assert_eq!(size, TerminalSize::new(24, 80));
    }

    #[test]
    fn grid_size_is_at_least_one_cell() {
        assert_eq!(metrics().grid_size(Size::ZERO), TerminalSize::new(1, 1));
    }

    #[test]
    fn selection_points_are_clamped_to_the_grid() {
        let screen = woop_term::Terminal::new(TerminalSize::new(24, 80)).screen();
        let palette = Palette::default();
        let cache = canvas::Cache::new();
        let view = TerminalView {
            screen: &screen,
            palette: &palette,
            metrics: metrics(),
            keys: keys::KeyContext {
                mode: woop_term::TerminalMode::default(),
                has_selection: false,
                shortcuts: keys::Shortcuts::platform(),
            },
            cache: &cache,
        };

        let inside = view.selection_point(Point::new(
            PADDING + 8.0 * 3.0 + 5.0,
            PADDING + 18.0 * 2.0 + 1.0,
        ));
        assert_eq!(
            inside,
            SelectionPoint {
                row: 2,
                col: 3,
                right_half: true
            }
        );

        let before = view.selection_point(Point::new(-50.0, -50.0));
        assert_eq!(before, SelectionPoint::new(0, 0));

        let after = view.selection_point(Point::new(5000.0, 5000.0));
        assert_eq!(
            after,
            SelectionPoint {
                row: 23,
                col: 79,
                right_half: true
            }
        );
    }
}
