//! The command editor's panel under the grid (ADR 0016).

use iced::widget::text_editor::{self, Binding, Content, KeyPress, Status};
use iced::widget::{Id, Space, column, container, operation, row, stack, text};
use iced::{Background, Border, Element, Length, Padding, Task};

use crate::Message;
use crate::gate::Status as GateStatus;
use crate::keys::{self, EditorContext, EditorKey, KeyContext};
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
    /// Colours.
    pub palette: &'a Palette,
    /// Font and cell size of the grid, shared by the editor.
    pub metrics: Metrics,
}

/// Lines of text the panel needs for `content`, within its limits.
pub fn lines_for(content: &Content) -> usize {
    content.line_count().clamp(1, MAX_LINES)
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
        palette,
        metrics,
    } = panel;
    let active = matches!(status, GateStatus::Active | GateStatus::Confirm);
    let cursor = content.cursor();
    let context = EditorContext {
        keys,
        has_selection: cursor.selection.is_some(),
        is_empty: content.is_empty(),
        on_first_line: cursor.position.line == 0,
        on_last_line: cursor.position.line + 1 >= content.line_count(),
    };
    let height = lines as f32 * metrics.cell_height + 2.0 * PADDING;
    let (foreground, background, selection) =
        (palette.foreground, palette.background, palette.selection);
    let warning = palette.failure;
    let muted = theme::dim(foreground, background);
    let placeholder = match status {
        GateStatus::Active | GateStatus::Confirm => "",
        GateStatus::HandedOff => "Editing in the shell's own line until the next prompt",
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
    let accent = match status {
        GateStatus::Confirm => warning,
        GateStatus::Active => palette.cursor,
        GateStatus::HandedOff | GateStatus::Running => muted,
    };
    let mut layers = stack![editor];
    if status == GateStatus::Confirm {
        // Drawn over the editor, so the grid keeps its size.
        layers = layers.push(
            container(
                text("The prompt changed while you typed: press Enter again to run")
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
    column![line, row![marker, layers]]
        .width(Length::Fill)
        .into()
}

fn binding(press: KeyPress, context: EditorContext) -> Option<Binding<Message>> {
    match keys::editor_key(&press.key, press.modifiers, context) {
        EditorKey::Edit => Binding::from_key_press(press),
        EditorKey::Ignore => None,
        EditorKey::Newline => Some(Binding::Enter),
        EditorKey::Copy => Some(Binding::Copy),
        EditorKey::Paste => Some(Binding::Paste),
        key => Some(Binding::Custom(Message::EditorKey(key))),
    }
}
