//! One terminal of the window: a shell session, its grid and its command
//! editor (ADR 0019).

use std::sync::Arc;

use iced::widget::text_editor::{self, Content};
use iced::widget::{canvas, column};
use iced::{Element, Length, Task, clipboard};
use woop_term::{
    Screen, SelectionKind, SelectionPoint, Session, Terminal, TerminalEvent, TerminalSize, input,
};

use crate::editor;
use crate::gate::{self, Consumed, Gate};
use crate::history::History;
use crate::keys::{self, EditorKey, KeyAction};
use crate::theme::Palette;
use crate::view::{Metrics, TerminalView};

/// Identifies a pane for as long as the window runs.
pub type PaneId = u64;

/// Messages handled by a pane.
#[derive(Debug, Clone)]
pub enum Message {
    /// A key was pressed on the grid.
    Key(KeyAction),
    /// The mouse wheel scrolled by a number of lines (positive is up).
    Scroll(i32),
    /// The grid area changed size.
    Resized(TerminalSize),
    /// Clipboard contents arrived for pasting.
    Pasted(Option<String>),
    /// The left button was pressed: start a new selection.
    SelectionStarted(SelectionPoint, SelectionKind),
    /// The mouse moved while selecting.
    SelectionExtended(SelectionPoint),
    /// A block's bar was clicked: select that block's output.
    SelectBlockOutput(u64),
    /// A block's bar was double-clicked: collapse or expand its output.
    ToggleCollapse(u64),
    /// A block's bar was Alt+clicked: copy its command.
    CopyBlockCommand(u64),
    /// The command editor's text or cursor changed through the mouse or an
    /// input method.
    EditorAction(text_editor::Action),
    /// A key was pressed in the command editor; what it means is decided
    /// when the message is handled ([`Pane::editor_press`]).
    EditorPress(text_editor::KeyPress),
    /// The command editor's panel was clicked: keep the keyboard on it.
    FocusEditor,
    /// The pane was right-clicked: paste the clipboard.
    PasteClicked,
}

impl Message {
    /// Whether the message comes from a click on the pane, which gives it
    /// the focus.
    pub fn is_click(&self) -> bool {
        matches!(
            self,
            Self::SelectionStarted(..)
                | Self::SelectBlockOutput(_)
                | Self::ToggleCollapse(_)
                | Self::CopyBlockCommand(_)
                | Self::FocusEditor
                | Self::PasteClicked
        )
    }

    /// Whether the message carries input for the shell or the editor typed
    /// or pasted with the keyboard. Such input is meant for the pane that
    /// has the focus; it is dropped if the focus moved before it was
    /// handled (ADR 0019).
    pub fn is_keyboard_input(&self) -> bool {
        matches!(
            self,
            Self::Key(_) | Self::EditorPress(_) | Self::EditorAction(_) | Self::Pasted(_)
        )
    }
}

/// A shell session and everything shown for it.
pub struct Pane {
    id: PaneId,
    session: Session,
    screen: Screen,
    size: TerminalSize,
    metrics: Metrics,
    cache: canvas::Cache,
    title: Option<String>,
    /// Text of the command editor (ADR 0016).
    editor: Content,
    /// The editor turned on at least once (the shell has integration), so
    /// its panel is shown from then on.
    integration: bool,
    /// Which prompt the editor takes the keys for.
    gate: Gate,
    /// Lines of text the editor's panel has room for. It grows while
    /// editing and shrinks back at a new prompt or once the editor is empty.
    panel_lines: usize,
    /// A message shown on the panel until the text changes.
    notice: Option<&'static str>,
}

impl Pane {
    /// Wraps a session that was just started, at `size`.
    pub fn new(id: PaneId, session: Session, size: TerminalSize, metrics: Metrics) -> Self {
        Self {
            id,
            screen: session.terminal().screen(),
            session,
            size,
            metrics,
            cache: canvas::Cache::new(),
            title: None,
            editor: Content::new(),
            integration: false,
            gate: Gate::default(),
            panel_lines: 1,
            notice: None,
        }
    }

    /// The title the running program set, if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref().filter(|title| !title.is_empty())
    }

    /// Draws the pane again with a new font, after the configuration
    /// changed; the grid is measured again on its next layout. Colours are
    /// read when drawing, so this also applies new colours.
    pub fn restyle(&mut self, metrics: Metrics) {
        self.metrics = metrics;
        self.cache.clear();
    }

    /// The size of the grid.
    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// Whether the shell has exited.
    pub fn is_closed(&self) -> bool {
        self.session.is_closed()
    }

    /// Whether closing the pane could end a running command, so the user
    /// must confirm. Without shell integration Woop cannot tell, so it
    /// always asks.
    pub fn is_running(&self) -> bool {
        !self.integration || self.session.terminal().command_running()
    }

    /// Gives the keyboard to the pane's editor, if it is active.
    pub fn focus(&self) -> Task<Message> {
        if self.editor_active() {
            editor::focus(self.id)
        } else {
            Task::none()
        }
    }

    /// Handles a message for this pane.
    pub fn update(&mut self, message: Message, history: &mut History) -> Task<Message> {
        // Clicking the grid takes the focus away from the editor; it is
        // given back so that typing keeps going to the editor.
        let clicked = message.is_click();
        let was_active = self.editor_active();
        let task = self.handle(message, history);
        self.after_change(task, was_active, clicked)
    }

    /// Applies output the shell produced. Called when the session reports
    /// output, also while the pane is not shown.
    pub fn process_output(&mut self) -> Task<Message> {
        let was_active = self.editor_active();
        let events = match self.session.pump() {
            Ok(events) => events,
            Err(error) => {
                eprintln!("woop: failed to process output: {error}");
                Vec::new()
            }
        };
        for event in events {
            match event {
                TerminalEvent::Title(title) => self.title = Some(title),
                TerminalEvent::ResetTitle => self.title = None,
                TerminalEvent::Bell | TerminalEvent::Reply(_) => {}
            }
        }
        self.refresh();
        self.after_change(Task::none(), was_active, false)
    }

    /// Updates the command editor's state after the screen or the editor
    /// changed.
    fn after_change(
        &mut self,
        task: Task<Message>,
        was_active: bool,
        clicked: bool,
    ) -> Task<Message> {
        let turned_on = self
            .gate
            .observe(self.screen.awaiting_command, self.screen.command_prompt);
        if turned_on {
            self.integration = true;
            self.notice = None;
            // The panel only shrinks here, never while a command starts.
            self.panel_lines = editor::lines_for(&self.editor);
        }
        if self.editor_active() != was_active {
            // The grid's cursor is hidden while the editor is active.
            self.cache.clear();
        }
        if self.editor_active() && (clicked || turned_on) {
            Task::batch([task, editor::focus(self.id)])
        } else {
            task
        }
    }

    fn handle(&mut self, message: Message, history: &mut History) -> Task<Message> {
        match message {
            Message::Key(KeyAction::Input(bytes)) => {
                self.scroll_to_bottom();
                self.write(&bytes);
            }
            Message::Key(KeyAction::Paste) => return clipboard::read().map(Message::Pasted),
            Message::Key(KeyAction::Copy) => return self.copy_grid_selection(),
            Message::SelectionStarted(point, kind) => {
                self.session.terminal_mut().start_selection(kind, point);
                self.refresh();
            }
            Message::SelectionExtended(point) => {
                self.session.terminal_mut().extend_selection(point);
                self.refresh();
            }
            Message::SelectBlockOutput(id) => {
                if self.session.terminal_mut().select_block_output(id) {
                    self.refresh();
                }
            }
            Message::ToggleCollapse(id) => {
                if self.session.terminal_mut().toggle_collapsed(id).is_some() {
                    self.refresh();
                }
            }
            Message::CopyBlockCommand(id) => {
                // Read now, from what is displayed, not from what the line
                // held when the command was submitted.
                if let Some(command) = self.session.terminal().block_command(id) {
                    return clipboard::write(command);
                }
            }
            Message::Key(KeyAction::JumpBlock(direction)) => {
                if self.session.terminal_mut().jump_to_block(direction) {
                    self.refresh();
                }
            }
            Message::Key(KeyAction::ScrollPages(pages)) => {
                self.scroll(pages * i32::from(self.size.rows.saturating_sub(1).max(1)));
            }
            // Handled by the window before reaching a pane.
            Message::Key(KeyAction::Workspace(_)) => {}
            Message::Scroll(lines) => self.scroll(lines),
            Message::Resized(size) => {
                self.size = size;
                if let Err(error) = self.session.resize(size) {
                    eprintln!("woop: failed to resize: {error}");
                }
                self.refresh();
            }
            Message::Pasted(Some(text)) => {
                if self.editor_active() {
                    self.paste_into_editor(text, history);
                } else {
                    self.paste_into_shell(&text);
                }
            }
            // The clipboard holds no text (an image, for example): say so,
            // rather than doing nothing.
            Message::Pasted(None) => {
                if self.editor_active() {
                    self.notice = Some("The clipboard has no text to paste");
                }
            }
            Message::EditorAction(action) => {
                if self.editor_active() {
                    self.edit(action, history);
                } else if let text_editor::Action::Edit(text_editor::Edit::Paste(text)) = action {
                    // Text from an input method, committed just after the
                    // editor turned off: it belongs to the shell now.
                    self.paste_into_shell(&text);
                }
            }
            Message::EditorPress(press) => return self.editor_press(press, history),
            Message::FocusEditor => return self.focus(),
            Message::PasteClicked => return clipboard::read().map(Message::Pasted),
        }
        Task::none()
    }

    /// What key presses mean right now.
    fn key_context(&self) -> keys::KeyContext {
        let terminal = self.session.terminal();
        keys::KeyContext {
            mode: terminal.mode(),
            has_selection: terminal.has_selection(),
            shortcuts: keys::Shortcuts::platform(),
            editor_active: self.editor_active(),
        }
    }

    /// Draws the pane. Only the `focused` pane takes keys.
    pub fn view<'a>(&'a self, palette: &'a Palette, focused: bool) -> Element<'a, Message> {
        let keys = self.key_context();
        let grid = canvas(TerminalView {
            screen: &self.screen,
            palette,
            metrics: self.metrics,
            keys,
            takes_keys: focused,
            cache: &self.cache,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        if !self.panel_visible() {
            return grid.into();
        }
        let panel = editor::panel(editor::Panel {
            id: self.id,
            content: &self.editor,
            status: self.gate.status(self.screen.command_prompt),
            takes_keys: focused,
            lines: self.panel_lines,
            keys,
            notice: self.notice,
            palette,
            metrics: self.metrics,
        });
        column![grid, panel].into()
    }

    /// The editor takes the keys ([`Gate`]).
    fn editor_active(&self) -> bool {
        self.gate.is_active()
    }

    /// The editor's panel is shown once the shell reported a prompt, except
    /// while a full-screen application uses the pane.
    fn panel_visible(&self) -> bool {
        self.integration && !self.session.terminal().mode().alt_screen
    }

    /// Applies an editing action to the editor's text.
    fn edit(&mut self, action: text_editor::Action, history: &mut History) {
        if action.is_edit() {
            self.text_changed(history);
        }
        self.editor.perform(action);
        self.grow_panel();
    }

    /// Replaces the editor's text, with the cursor at its end. A pending
    /// confirmation no longer applies; a walk through the history goes on.
    fn set_editor_text(&mut self, text: &str) {
        self.gate.edited();
        self.notice = None;
        self.editor = Content::with_text(text);
        self.editor
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        self.grow_panel();
    }

    /// The editor's text changed: a walk through the history, a pending
    /// confirmation and a notice no longer apply.
    fn text_changed(&mut self, history: &mut History) {
        history.stop_walking();
        self.gate.edited();
        self.notice = None;
    }

    /// The panel grows with the text; it shrinks only at a new prompt, so
    /// the grid is not resized while editing or when a command starts.
    /// Once the editor is empty again, the panel goes back to one line.
    fn grow_panel(&mut self) {
        self.panel_lines = if self.editor.is_empty() {
            1
        } else {
            self.panel_lines.max(editor::lines_for(&self.editor))
        };
    }

    /// Pastes text into the shell, as on the grid.
    fn paste_into_shell(&mut self, text: &str) {
        let bytes = input::encode_paste(text, self.session.terminal().mode());
        self.scroll_to_bottom();
        self.write(&bytes);
    }

    /// Pastes text into the editor, within the editor's limits: laying out
    /// very long text makes every key slow. Larger text goes to the shell's
    /// own line as a paste when the editor is empty, and is refused
    /// otherwise. Control characters are kept here and removed when the
    /// command is sent.
    fn paste_into_editor(&mut self, text: String, history: &mut History) {
        if editor::fits(&self.editor, &text) {
            self.edit(
                text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text))),
                history,
            );
        } else if self.editor.is_empty() {
            self.paste_into_shell(&text);
            self.gate.consume(Consumed::HandedOff);
        } else {
            self.notice =
                Some("Too long to edit here: clear the editor to paste it into the shell");
        }
    }

    /// Handles a key pressed in the editor, with the state at this moment.
    fn editor_press(
        &mut self,
        mut press: text_editor::KeyPress,
        history: &mut History,
    ) -> Task<Message> {
        let keys = self.key_context();
        if !self.editor_active() {
            // Pressed just after the editor turned off (after Enter, before
            // the window was redrawn): the key goes to the shell as on the
            // grid, in order with the ones after it.
            let grid = keys::KeyContext {
                editor_active: false,
                ..keys
            };
            return match keys::key_press(&press.key, press.modifiers, press.text.as_deref(), grid) {
                Some(action) => self.handle(Message::Key(action), history),
                None => Task::none(),
            };
        }
        // Keys reach the editor even when a click elsewhere took its focus;
        // the focus is given back so its cursor shows again.
        let refocus = !matches!(press.status, text_editor::Status::Focused { .. });
        press.status = text_editor::Status::Focused { is_hovered: false };

        let context = editor::context(&self.editor, keys);
        let key = keys::editor_key(&press.key, press.modifiers, press.text.as_deref(), context);
        let task = match key {
            EditorKey::Edit => match text_editor::Binding::from_key_press(press) {
                Some(binding) => self.apply_binding(binding, history),
                None => Task::none(),
            },
            key => self.editor_key(key, history),
        };
        if refocus {
            Task::batch([task, editor::focus(self.id)])
        } else {
            task
        }
    }

    /// Performs one of the editor's own key bindings.
    fn apply_binding(
        &mut self,
        binding: text_editor::Binding<Message>,
        history: &mut History,
    ) -> Task<Message> {
        use text_editor::{Action, Binding, Edit};
        match binding {
            Binding::Copy => return self.copy_editor_selection(),
            Binding::Cut => {
                let task = self.copy_editor_selection();
                if self.editor.selection().is_some() {
                    self.edit(Action::Edit(Edit::Delete), history);
                }
                return task;
            }
            Binding::Paste => return clipboard::read().map(Message::Pasted),
            Binding::Move(motion) => self.edit(Action::Move(motion), history),
            Binding::Select(motion) => self.edit(Action::Select(motion), history),
            Binding::SelectWord => self.edit(Action::SelectWord, history),
            Binding::SelectLine => self.edit(Action::SelectLine, history),
            Binding::SelectAll => self.edit(Action::SelectAll, history),
            Binding::Insert(ch) => self.edit(Action::Edit(Edit::Insert(ch)), history),
            Binding::Enter => self.edit(Action::Edit(Edit::Enter), history),
            Binding::Backspace => self.edit(Action::Edit(Edit::Backspace), history),
            Binding::Delete => self.edit(Action::Edit(Edit::Delete), history),
            Binding::Sequence(bindings) => {
                return Task::batch(
                    bindings
                        .into_iter()
                        .map(|binding| self.apply_binding(binding, history))
                        .collect::<Vec<_>>(),
                );
            }
            // The editor keeps the focus while it is active; Woop produces
            // no custom bindings.
            Binding::Unfocus | Binding::Custom(_) => {}
        }
        Task::none()
    }

    fn copy_editor_selection(&self) -> Task<Message> {
        self.editor
            .selection()
            .map_or_else(Task::none, clipboard::write)
    }

    fn editor_key(&mut self, key: EditorKey, history: &mut History) -> Task<Message> {
        let mode = self.session.terminal().mode();
        match key {
            EditorKey::Submit => {
                // Output may have moved the cursor since the editor turned
                // on; if the shell's line might hold text, the user confirms.
                if self.gate.submit(self.screen.awaiting_command) == gate::Submit::Confirm {
                    return Task::none();
                }
                let text = self.editor.text();
                self.scroll_to_bottom();
                // If the input cannot be queued, the text stays in the
                // editor, so nothing typed is lost.
                if self.write(&input::encode_command(&text, mode)) {
                    history.push(&text);
                    self.gate.consume(Consumed::Submitted);
                    self.editor = Content::new();
                    self.notice = None;
                }
            }
            EditorKey::HandOff(key) => {
                let text = self.editor.text();
                // A line break would run part of the command, so multi-line
                // text is not handed off.
                if let Some(mut bytes) = input::encode_text_for_line(&text, mode) {
                    bytes.extend_from_slice(&key);
                    self.scroll_to_bottom();
                    if self.write(&bytes) {
                        history.stop_walking();
                        self.gate.consume(Consumed::HandedOff);
                        self.editor = Content::new();
                        self.notice = None;
                    }
                }
            }
            EditorKey::HistoryPrevious => {
                let current = self.editor.text();
                if let Some(command) = history.previous(&current).map(str::to_owned) {
                    self.set_editor_text(&command);
                }
            }
            EditorKey::HistoryNext => {
                if let Some(command) = history.next().map(str::to_owned) {
                    self.set_editor_text(&command);
                }
            }
            EditorKey::Clear => {
                history.stop_walking();
                self.set_editor_text("");
            }
            EditorKey::Shell(bytes) => {
                self.write(&bytes);
            }
            EditorKey::CopyGrid => return self.copy_grid_selection(),
            EditorKey::Copy => return self.copy_editor_selection(),
            EditorKey::Paste => return clipboard::read().map(Message::Pasted),
            EditorKey::Newline => {
                self.edit(text_editor::Action::Edit(text_editor::Edit::Enter), history);
            }
            EditorKey::Type(ch) => {
                self.edit(
                    text_editor::Action::Edit(text_editor::Edit::Insert(ch)),
                    history,
                );
            }
            // Edit is resolved by the caller; Ignore does nothing.
            EditorKey::Edit | EditorKey::Ignore => {}
        }
        Task::none()
    }

    fn copy_grid_selection(&mut self) -> Task<Message> {
        // The selection is cleared once copied, so that a second Ctrl+C on
        // Windows interrupts the shell as usual.
        if let Some(text) = self.session.terminal().selection_text() {
            self.session.terminal_mut().clear_selection();
            self.refresh();
            return clipboard::write(text);
        }
        Task::none()
    }

    fn scroll(&mut self, lines: i32) {
        let terminal = self.session.terminal();
        if terminal.mode().alt_screen {
            // Full-screen applications have no history; send arrow keys so
            // they can scroll their own content.
            let key = if lines > 0 {
                input::Key::Up
            } else {
                input::Key::Down
            };
            if let Some(bytes) =
                input::encode_key(key, input::Modifiers::default(), terminal.mode())
            {
                self.write(&bytes.repeat(lines.unsigned_abs() as usize));
            }
        } else {
            self.session.terminal_mut().scroll(lines);
            self.refresh();
        }
    }

    /// Brings the view back to the most recent output, as typing does.
    fn scroll_to_bottom(&mut self) {
        if self.session.terminal().scroll_offset() > 0 {
            self.session.terminal_mut().scroll_to_bottom();
            self.refresh();
        }
    }

    /// Queues input for the shell. Returns whether it was accepted.
    fn write(&mut self, bytes: &[u8]) -> bool {
        match self.session.write(bytes) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("woop: failed to send input: {error}");
                false
            }
        }
    }

    fn refresh(&mut self) {
        self.screen = Terminal::screen(self.session.terminal());
        self.cache.clear();
    }
}
