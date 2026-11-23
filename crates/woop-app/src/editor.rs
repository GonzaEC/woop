//! The command editor's panel under the grid (ADR 0016).

use iced::widget::text_editor::{self, Binding, Content, KeyPress, Status};
use iced::widget::{Id, Space, column, container, mouse_area, operation, row, stack, text};
use iced::{Background, Border, Element, Length, Padding, Task};

use crate::Message;
use crate::gate::Status as GateStatus;
use crate::keys::{self, EditorContext, EditorKey, KeyAction, KeyContext};
use crate::theme::{self, Palette};
use crate::view::Metrics;

/// Most lines of text the panel grows to; longer text scrolls inside it.
const MAX_LINES: usize = 8;

/// Space around the text inside the panel, in logical pixels.
const PADDING: f32 = 6.0;

const EDITOR_ID: &str = "woop-command-editor";

/// Shown in front of the editor's text.
const MARKER: &str = "\u{203a}";

/// What the panel shows.
pub struct Panel<'a> {
    /// The editor's text.
    pub content: &'a Content,
    /// Whether the editor takes the keys, and what it shows otherwise.
    pub status: GateStatus,
    /// Lines of text the panel has room for.
    pub lines: usize,
    /// How keys are interpreted on the grid.
    pub keys: KeyContext,
    /// A message to show over the editor, if any.
    pub notice: Option<&'a str>,
    /// Colours.
    pub palette: &'a Palette,
    /// Font and cell size of the grid, shared by the editor.
    pub metrics: Metrics,
}

/// Most text the editor holds, in bytes. Laying out text costs time on
/// every key, so very large pastes go to the shell instead (measured with
/// one line: 26 ms per key at 64 KiB, over half a second at 1 MiB).
const MAX_TEXT_BYTES: usize = 64 * 1024;

/// Longest line the editor holds, in bytes: a line is laid out again as a
/// whole on every change within it.
const MAX_LINE_BYTES: usize = 4 * 1024;

/// Lines of text the panel needs for `content`, within its limits.
pub fn lines_for(content: &Content) -> usize {
    content.line_count().clamp(1, MAX_LINES)
}

/// Whether pasting `text` keeps the editor within its limits.
pub fn fits(content: &Content, text: &str) -> bool {
    fits_text(&content.text(), text)
}

fn fits_text(current: &str, pasted: &str) -> bool {
    let longest = |text: &str| text.lines().map(str::len).max().unwrap_or(0);
    current.len() + pasted.len() <= MAX_TEXT_BYTES
        && longest(current) + longest(pasted) <= MAX_LINE_BYTES
}

/// Gives the keyboard focus to the editor.
pub fn focus() -> Task<Message> {
    operation::focus(Id::new(EDITOR_ID))
}

/// Builds the panel. While a command runs, the editor is shown disabled.
pub fn panel(panel: Panel<'_>) -> Element<'_, Message> {
    let Panel {
        content,
        status,
        lines,
        keys,
        notice,
        palette,
        metrics,
    } = panel;
    let active = matches!(status, GateStatus::Active | GateStatus::Confirm);
    let context = context(content, keys);
    let height = lines as f32 * metrics.cell_height + 2.0 * PADDING;
    let (foreground, background, selection) =
        (palette.foreground, palette.background, palette.selection);
    let warning = palette.failure;
    let muted = theme::dim(foreground, background);
    let placeholder = match status {
        GateStatus::Active | GateStatus::Confirm => "",
        GateStatus::ShellLine => "Editing in the shell's own line until the next prompt",
        GateStatus::Running => "Running…",
    };

    let mut editor = iced::widget::text_editor(content)
        .id(Id::new(EDITOR_ID))
        .placeholder(placeholder)
        .font(metrics.font)
        .size(metrics.font_size)
        .line_height(text::LineHeight::Absolute(metrics.cell_height.into()))
        .padding(PADDING)
        .height(Length::Fixed(height))
        .wrapping(text::Wrapping::WordOrGlyph)
        .style(move |_theme, status| text_editor::Style {
            background: Background::Color(background),
            border: Border::default(),
            placeholder: muted,
            value: if matches!(status, Status::Disabled) {
                muted
            } else {
                foreground
            },
            selection,
        });
    if active {
        editor = editor
            .on_action(Message::EditorAction)
            .key_binding(move |press| binding(press, context));
    }
    let message = match (status, notice) {
        (GateStatus::Confirm, _) => {
            Some("The prompt changed while you typed: press Enter again to run")
        }
        (_, notice) => notice,
    };
    let accent = match status {
        _ if message.is_some() => warning,
        GateStatus::Active | GateStatus::Confirm => palette.cursor,
        GateStatus::ShellLine | GateStatus::Running => muted,
    };
    let mut layers = stack![editor];
    if let Some(message) = message {
        // Drawn over the editor, so the grid keeps its size.
        layers = layers.push(
            container(
                text(message)
                    .font(metrics.font)
                    .size(metrics.font_size)
                    .color(warning),
            )
            .padding(PADDING)
            .align_right(Length::Fill),
        );
    }
    // A marker in front of the text shows where typing goes, and a line
    // separates the panel from the grid.
    let marker = container(
        text(MARKER)
            .font(metrics.font)
            .size(metrics.font_size)
            .line_height(text::LineHeight::Absolute(metrics.cell_height.into()))
            .color(accent),
    )
    .padding(Padding {
        left: PADDING,
        top: PADDING,
        ..Padding::ZERO
    });
    let line = container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(accent)),
            ..container::Style::default()
        });
    // Any click on the panel, also outside the text, keeps the keyboard on
    // the editor. A right click pastes, as on the grid: into the editor
    // while it is active, into the shell otherwise.
    mouse_area(column![line, row![marker, layers]].width(Length::Fill))
        .on_press(Message::FocusEditor)
        .on_right_press(Message::Key(KeyAction::Paste))
        .into()
}

/// The editor's state needed to interpret a key press.
pub fn context(content: &Content, keys: KeyContext) -> EditorContext {
    let cursor = content.cursor();
    EditorContext {
        keys,
        has_selection: cursor.selection.is_some(),
        is_empty: content.is_empty(),
        on_first_line: cursor.position.line == 0,
        on_last_line: cursor.position.line + 1 >= content.line_count(),
    }
}

/// Passes key presses on to the application, which decides what they mean
/// with the state it has when it handles them: a key typed just after Enter
/// may arrive once the editor is no longer active, and must then go to the
/// shell like any key on the grid. Keys left to the grid's own shortcuts
/// are not taken.
fn binding(press: KeyPress, context: EditorContext) -> Option<Binding<Message>> {
    let key = keys::editor_key(&press.key, press.modifiers, press.text.as_deref(), context);
    (key != EditorKey::Ignore).then(|| Binding::Custom(Message::EditorPress(press)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pastes_within_the_limits_fit() {
        assert!(fits_text("", "git status"));
        assert!(fits_text("echo ", &"x".repeat(MAX_LINE_BYTES - 5)));
        let lines = "y".repeat(1000) + "\n";
        assert!(fits_text("", &lines.repeat(60)), "many short lines");
    }

    #[test]
    fn very_long_text_or_lines_do_not_fit() {
        // A minified file or base64: one huge line.
        assert!(!fits_text("", &"x".repeat(MAX_LINE_BYTES + 1)));
        assert!(!fits_text("echo ", &"x".repeat(MAX_LINE_BYTES)));
        let lines = "y".repeat(100) + "\n";
        assert!(!fits_text("", &lines.repeat(MAX_TEXT_BYTES / 100)));
    }
}
