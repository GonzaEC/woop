//! The command editor's panel under the grid (ADR 0016).

use iced::widget::text_editor::{self, Binding, Content, KeyPress, Status};
use iced::widget::{Id, Space, column, container, mouse_area, operation, row, stack, text};
use iced::{Background, Border, Element, Length, Padding, Task};

use crate::completion::{Kind, List};
use crate::gate::Status as GateStatus;
use crate::keys::{self, EditorContext, EditorKey, KeyContext};
use crate::pane::Message;
use crate::pane::PaneId;
use crate::theme::{self, Palette};
use crate::view::Metrics;

/// Most lines of text the panel grows to; longer text scrolls inside it.
const MAX_LINES: usize = 8;

/// Space around the text inside the panel, in logical pixels.
const PADDING: f32 = 6.0;

/// Shown in front of the editor's text.
const MARKER: &str = "\u{203a}";

/// What the panel shows.
pub struct Panel<'a> {
    /// The pane the editor belongs to.
    pub id: PaneId,
    /// The editor's text.
    pub content: &'a Content,
    /// Whether the editor takes the keys, and what it shows otherwise.
    pub status: GateStatus,
    /// The pane has the keyboard focus; the editors of other panes are
    /// shown but take no keys.
    pub takes_keys: bool,
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
    /// The completion list is open, so its keys go to it.
    pub completing: bool,
}

/// Most entries of the completion list shown at once.
const LIST_ROWS: usize = 8;

/// Builds the completion list, drawn over the bottom of the grid (ADR
/// 0023). Clicking an entry inserts it.
pub fn completion_list<'a>(
    list: &'a List,
    palette: &'a Palette,
    metrics: Metrics,
) -> Element<'a, Message> {
    let first = list.first_shown(LIST_ROWS);
    let (foreground, background) = (palette.foreground, palette.background);
    let muted = theme::dim(foreground, background);
    let selection = palette.selection;
    let rows = list
        .candidates
        .iter()
        .enumerate()
        .skip(first)
        .take(LIST_ROWS)
        .map(|(index, candidate)| {
            let kind = match candidate.kind {
                Kind::Directory => "dir ",
                Kind::File => "file",
                Kind::History => "hist",
                Kind::Subcommand => "cmd ",
                Kind::Option => "opt ",
                Kind::Value => "val ",
            };
            let chosen = index == list.selected;
            let line = row![
                text(kind)
                    .font(metrics.font)
                    .size(metrics.font_size)
                    .color(muted),
                text(candidate.label.as_str())
                    .font(metrics.font)
                    .size(metrics.font_size)
                    .color(foreground)
                    .wrapping(text::Wrapping::None),
                text(candidate.description.as_str())
                    .font(metrics.font)
                    .size(metrics.font_size)
                    .color(muted)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(metrics.font_size);
            let entry = container(line)
                .padding(Padding::from([1.0, PADDING]))
                .width(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(Background::Color(if chosen {
                        selection
                    } else {
                        background
                    })),
                    ..container::Style::default()
                });
            mouse_area(entry)
                .on_press(Message::CompletionClicked(index))
                .into()
        });
    let count = text(format!("{}/{}", list.selected + 1, list.candidates.len()))
        .font(metrics.font)
        .size(metrics.font_size)
        .color(muted);
    let entries = column(rows).push(container(count).padding(Padding::from([1.0, PADDING])));
    container(entries)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(background)),
            border: Border {
                color: muted,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
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

/// Whether the editor can hold `text` on its own.
pub fn holds(text: &str) -> bool {
    fits_text("", text)
}

fn fits_text(current: &str, pasted: &str) -> bool {
    let longest = |text: &str| text.lines().map(str::len).max().unwrap_or(0);
    current.len() + pasted.len() <= MAX_TEXT_BYTES
        && longest(current) + longest(pasted) <= MAX_LINE_BYTES
}

/// The widget id of a pane's editor.
fn editor_id(pane: PaneId) -> Id {
    Id::from(format!("woop-command-editor-{pane}"))
}

/// Gives the keyboard focus to a pane's editor.
pub fn focus(pane: PaneId) -> Task<Message> {
    operation::focus(editor_id(pane))
}

/// Builds the panel. While a command runs, the editor is shown disabled.
pub fn panel(panel: Panel<'_>) -> Element<'_, Message> {
    let Panel {
        id,
        content,
        status,
        takes_keys,
        lines,
        keys,
        notice,
        palette,
        metrics,
        completing,
    } = panel;
    let active = takes_keys && matches!(status, GateStatus::Active | GateStatus::Confirm);
    let context = context(content, keys, completing);
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
        .id(editor_id(id))
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
            .key_binding(move |press| binding(press, context.clone()));
    }
    let message = match (status, notice) {
        (GateStatus::Confirm, _) => {
            Some("The prompt changed while you typed: press Enter again to run")
        }
        (_, notice) => notice,
    };
    let accent = match status {
        _ if message.is_some() => warning,
        // Only the focused pane's editor is highlighted.
        _ if !takes_keys => muted,
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
        .on_right_press(Message::PasteClicked)
        .into()
}

/// The editor's state needed to interpret a key press.
pub fn context(content: &Content, keys: KeyContext, completing: bool) -> EditorContext {
    let cursor = content.cursor();
    EditorContext {
        keys,
        has_selection: cursor.selection.is_some(),
        is_empty: content.is_empty(),
        on_first_line: cursor.position.line == 0,
        on_last_line: cursor.position.line + 1 >= content.line_count(),
        completing,
    }
}

/// Passes key presses on to the application, which decides what they mean
/// with the state it has when it handles them: a key typed just after Enter
/// may arrive once the editor is no longer active, and must then go to the
/// shell like any key on the grid. Keys left to the grid's own shortcuts
/// are not taken.
fn binding(press: KeyPress, context: EditorContext) -> Option<Binding<Message>> {
    let key = keys::editor_key(&press.key, press.modifiers, press.text.as_deref(), context);
    (key != EditorKey::Ignore).then_some(Binding::Custom(Message::EditorPress(press)))
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
