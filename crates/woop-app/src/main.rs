//! Woop application entry point.

mod editor;
mod gate;
mod history;
mod keys;
mod theme;
mod view;

use std::sync::Arc;

use iced::futures::channel::mpsc::{self, UnboundedSender};
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::widget::text_editor::{self, Content};
use iced::widget::{canvas, column};
use iced::{Element, Length, Subscription, Task, clipboard};
use woop_term::{
    Screen, SelectionKind, SelectionPoint, Session, ShellCommand, Terminal, TerminalEvent,
    TerminalSize, input, shell_integration,
};

use gate::{Consumed, Gate};
use history::History;
use keys::{EditorKey, KeyAction};
use theme::Palette;
use view::{Metrics, TerminalView};

const FONT_SIZE: f32 = 14.0;
const INITIAL_SIZE: TerminalSize = TerminalSize {
    rows: 30,
    cols: 100,
};
const APP_NAME: &str = "Woop";

fn main() -> iced::Result {
    iced::application(Woop::new, Woop::update, Woop::view)
        .title(Woop::title)
        .subscription(Woop::subscription)
        .window_size((960.0, 600.0))
        .centered()
        .run()
}

/// Messages handled by the application.
#[derive(Debug, Clone)]
pub enum Message {
    /// The output listener is running; the session can be started.
    ListenerReady(UnboundedSender<()>),
    /// The shell produced output.
    Output,
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
    /// when the message is handled ([`Woop::editor_press`]).
    EditorPress(text_editor::KeyPress),
    /// The command editor's panel was clicked: keep the keyboard on it.
    FocusEditor,
}

struct Woop {
    session: Option<Session>,
    screen: Screen,
    size: TerminalSize,
    palette: Palette,
    metrics: Metrics,
    cache: canvas::Cache,
    title: Option<String>,
    /// Text of the command editor (ADR 0016).
    editor: Content,
    history: History,
    /// The editor turned on at least once (the shell has integration), so
    /// its panel is shown from then on.
    integration: bool,
    /// Which prompt the editor takes the keys for.
    gate: Gate,
    /// Lines of text the editor's panel has room for. It grows while
    /// editing and shrinks back only at a new prompt.
    panel_lines: usize,
    /// A message shown on the panel until the text changes.
    notice: Option<&'static str>,
}

impl Woop {
    fn new() -> Self {
        let size = INITIAL_SIZE;
        Self {
            session: None,
            screen: woop_term::Terminal::new(size).screen(),
            size,
            palette: Palette::default(),
            metrics: Metrics::measure(view::default_font(), FONT_SIZE),
            cache: canvas::Cache::new(),
            title: None,
            editor: Content::new(),
            history: History::default(),
            integration: false,
            gate: Gate::default(),
            panel_lines: 1,
            notice: None,
        }
    }

    fn title(&self) -> String {
        match &self.title {
            Some(title) if !title.is_empty() => format!("{title} - {APP_NAME}"),
            _ => APP_NAME.to_owned(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        // Clicking the grid takes the focus away from the editor; it is
        // given back so that typing keeps going to the editor.
        let clicked = matches!(
            message,
            Message::SelectionStarted(..)
                | Message::SelectBlockOutput(_)
                | Message::ToggleCollapse(_)
                | Message::CopyBlockCommand(_)
        );
        let was_active = self.editor_active();
        let task = self.handle(message);

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
            Task::batch([task, editor::focus()])
        } else {
            task
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ListenerReady(sender) => self.start_session(sender),
            Message::Output => return self.process_output(),
            Message::Key(KeyAction::Input(bytes)) => {
                self.scroll_to_bottom();
                self.write(&bytes);
            }
            Message::Key(KeyAction::Paste) => return clipboard::read().map(Message::Pasted),
            Message::Key(KeyAction::Copy) => return self.copy_grid_selection(),
            Message::SelectionStarted(point, kind) => {
                if let Some(session) = &mut self.session {
                    session.terminal_mut().start_selection(kind, point);
                    self.refresh();
                }
            }
            Message::SelectionExtended(point) => {
                if let Some(session) = &mut self.session {
                    session.terminal_mut().extend_selection(point);
                    self.refresh();
                }
            }
            Message::SelectBlockOutput(id) => {
                if let Some(session) = &mut self.session
                    && session.terminal_mut().select_block_output(id)
                {
                    self.refresh();
                }
            }
            Message::ToggleCollapse(id) => {
                if let Some(session) = &mut self.session
                    && session.terminal_mut().toggle_collapsed(id).is_some()
                {
                    self.refresh();
                }
            }
            Message::CopyBlockCommand(id) => {
                // Read now, from what is displayed, not from what the line
                // held when the command was submitted.
                if let Some(command) = self
                    .session
                    .as_ref()
                    .and_then(|session| session.terminal().block_command(id))
                {
                    return clipboard::write(command);
                }
            }
            Message::Key(KeyAction::JumpBlock(direction)) => {
                if let Some(session) = &mut self.session
                    && session.terminal_mut().jump_to_block(direction)
                {
                    self.refresh();
                }
            }
            Message::Key(KeyAction::ScrollPages(pages)) => {
                self.scroll(pages * i32::from(self.size.rows.saturating_sub(1).max(1)));
            }
            Message::Scroll(lines) => self.scroll(lines),
            Message::Resized(size) => {
                self.size = size;
                if let Some(session) = &mut self.session {
                    if let Err(error) = session.resize(size) {
                        eprintln!("woop: failed to resize: {error}");
                    }
                    self.refresh();
                }
            }
            Message::Pasted(Some(text)) => {
                if self.editor_active() {
                    self.paste_into_editor(text);
                } else {
                    self.paste_into_shell(&text);
                }
            }
            Message::Pasted(None) => {}
            Message::EditorAction(action) => {
                if self.editor_active() {
                    self.edit(action);
                } else if let text_editor::Action::Edit(text_editor::Edit::Paste(text)) = action {
                    // Text from an input method, committed just after the
                    // editor turned off: it belongs to the shell now.
                    self.paste_into_shell(&text);
                }
            }
            Message::EditorPress(press) => return self.editor_press(press),
            Message::FocusEditor => {
                if self.editor_active() {
                    return editor::focus();
                }
            }
        }
        Task::none()
    }

    /// What key presses mean right now.
    fn key_context(&self) -> keys::KeyContext {
        let terminal = self.session.as_ref().map(Session::terminal);
        keys::KeyContext {
            mode: terminal.map(Terminal::mode).unwrap_or_default(),
            has_selection: terminal.is_some_and(Terminal::has_selection),
            shortcuts: keys::Shortcuts::platform(),
            editor_active: self.editor_active(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let keys = self.key_context();
        let grid = canvas(TerminalView {
            screen: &self.screen,
            palette: &self.palette,
            metrics: self.metrics,
            keys,
            cache: &self.cache,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        if !self.panel_visible() {
            return grid.into();
        }
        let panel = editor::panel(editor::Panel {
            content: &self.editor,
            status: self.gate.status(self.screen.command_prompt),
            lines: self.panel_lines,
            keys,
            notice: self.notice,
            palette: &self.palette,
            metrics: self.metrics,
        });
        column![grid, panel].into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(output_listener)
    }

    /// The editor takes the keys ([`Gate`]).
    fn editor_active(&self) -> bool {
        self.session.is_some() && self.gate.is_active()
    }

    /// The editor's panel is shown once the shell reported a prompt, except
    /// while a full-screen application uses the window.
    fn panel_visible(&self) -> bool {
        self.integration
            && !self
                .session
                .as_ref()
                .is_some_and(|session| session.terminal().mode().alt_screen)
    }

    /// Applies an editing action to the editor's text.
    fn edit(&mut self, action: text_editor::Action) {
        if action.is_edit() {
            self.text_changed();
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
    fn text_changed(&mut self) {
        self.history.stop_walking();
        self.gate.edited();
        self.notice = None;
    }

    /// The panel grows with the text; it shrinks only at a new prompt, so
    /// the grid is not resized while editing or when a command starts.
    fn grow_panel(&mut self) {
        self.panel_lines = self.panel_lines.max(editor::lines_for(&self.editor));
    }

    /// Pastes text into the shell, as on the grid.
    fn paste_into_shell(&mut self, text: &str) {
        if let Some(session) = &self.session {
            let bytes = input::encode_paste(text, session.terminal().mode());
            self.scroll_to_bottom();
            self.write(&bytes);
        }
    }

    /// Pastes text into the editor, within the editor's limits: laying out
    /// very long text makes every key slow. Larger text goes to the shell's
    /// own line as a paste when the editor is empty, and is refused
    /// otherwise. Control characters are kept here and removed when the
    /// command is sent.
    fn paste_into_editor(&mut self, text: String) {
        if editor::fits(&self.editor, &text) {
            self.edit(text_editor::Action::Edit(text_editor::Edit::Paste(
                Arc::new(text),
            )));
        } else if self.editor.is_empty() {
            self.paste_into_shell(&text);
            self.gate.consume(Consumed::HandedOff);
        } else {
            self.notice =
                Some("Too long to edit here: clear the editor to paste it into the shell");
        }
    }

    /// Handles a key pressed in the editor, with the state at this moment.
    fn editor_press(&mut self, mut press: text_editor::KeyPress) -> Task<Message> {
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
                Some(action) => self.handle(Message::Key(action)),
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
                Some(binding) => self.apply_binding(binding),
                None => Task::none(),
            },
            key => self.editor_key(key),
        };
        if refocus {
            Task::batch([task, editor::focus()])
        } else {
            task
        }
    }

    /// Performs one of the editor's own key bindings.
    fn apply_binding(&mut self, binding: text_editor::Binding<Message>) -> Task<Message> {
        use text_editor::{Action, Binding, Edit};
        match binding {
            Binding::Copy => return self.copy_editor_selection(),
            Binding::Cut => {
                let task = self.copy_editor_selection();
                if self.editor.selection().is_some() {
                    self.edit(Action::Edit(Edit::Delete));
                }
                return task;
            }
            Binding::Paste => return clipboard::read().map(Message::Pasted),
            Binding::Move(motion) => self.edit(Action::Move(motion)),
            Binding::Select(motion) => self.edit(Action::Select(motion)),
            Binding::SelectWord => self.edit(Action::SelectWord),
            Binding::SelectLine => self.edit(Action::SelectLine),
            Binding::SelectAll => self.edit(Action::SelectAll),
            Binding::Insert(ch) => self.edit(Action::Edit(Edit::Insert(ch))),
            Binding::Enter => self.edit(Action::Edit(Edit::Enter)),
            Binding::Backspace => self.edit(Action::Edit(Edit::Backspace)),
            Binding::Delete => self.edit(Action::Edit(Edit::Delete)),
            Binding::Sequence(bindings) => {
                return Task::batch(
                    bindings
                        .into_iter()
                        .map(|binding| self.apply_binding(binding))
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

    fn editor_key(&mut self, key: EditorKey) -> Task<Message> {
        let mode = self
            .session
            .as_ref()
            .map(|session| session.terminal().mode())
            .unwrap_or_default();
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
                    self.history.push(&text);
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
                        self.history.stop_walking();
                        self.gate.consume(Consumed::HandedOff);
                        self.editor = Content::new();
                        self.notice = None;
                    }
                }
            }
            EditorKey::HistoryPrevious => {
                let current = self.editor.text();
                if let Some(command) = self.history.previous(&current).map(str::to_owned) {
                    self.set_editor_text(&command);
                }
            }
            EditorKey::HistoryNext => {
                if let Some(command) = self.history.next().map(str::to_owned) {
                    self.set_editor_text(&command);
                }
            }
            EditorKey::Clear => {
                self.history.stop_walking();
                self.set_editor_text("");
            }
            EditorKey::Shell(bytes) => {
                self.write(&bytes);
            }
            EditorKey::CopyGrid => return self.copy_grid_selection(),
            EditorKey::Copy => return self.copy_editor_selection(),
            EditorKey::Paste => return clipboard::read().map(Message::Pasted),
            EditorKey::Newline => {
                self.edit(text_editor::Action::Edit(text_editor::Edit::Enter));
            }
            EditorKey::Type(ch) => {
                self.edit(text_editor::Action::Edit(text_editor::Edit::Insert(ch)));
            }
            // Edit is resolved by the caller; Ignore does nothing.
            EditorKey::Edit | EditorKey::Ignore => {}
        }
        Task::none()
    }

    fn copy_grid_selection(&mut self) -> Task<Message> {
        // The selection is cleared once copied, so that a second Ctrl+C on
        // Windows interrupts the shell as usual.
        if let Some(session) = &mut self.session
            && let Some(text) = session.terminal().selection_text()
        {
            session.terminal_mut().clear_selection();
            self.refresh();
            return clipboard::write(text);
        }
        Task::none()
    }

    fn start_session(&mut self, sender: UnboundedSender<()>) {
        if self.session.is_some() {
            return;
        }
        let notify = move || {
            let _ = sender.unbounded_send(());
        };
        let command = shell_integration::apply(ShellCommand::default_shell());
        match Session::spawn_with_notifier(&command, self.size, notify) {
            Ok(session) => self.session = Some(session),
            Err(error) => {
                eprintln!("woop: failed to start the shell: {error}");
                std::process::exit(1);
            }
        }
    }

    fn process_output(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let events = match session.pump() {
            Ok(events) => events,
            Err(error) => {
                eprintln!("woop: failed to process output: {error}");
                Vec::new()
            }
        };
        if session.is_closed() {
            return iced::exit();
        }

        for event in events {
            match event {
                TerminalEvent::Title(title) => self.title = Some(title),
                TerminalEvent::ResetTitle => self.title = None,
                TerminalEvent::Bell | TerminalEvent::Reply(_) => {}
            }
        }
        self.refresh();
        Task::none()
    }

    fn scroll(&mut self, lines: i32) {
        let Some(session) = &mut self.session else {
            return;
        };
        if session.terminal().mode().alt_screen {
            // Full-screen applications have no history; send arrow keys so
            // they can scroll their own content.
            let key = if lines > 0 {
                input::Key::Up
            } else {
                input::Key::Down
            };
            let mode = session.terminal().mode();
            if let Some(bytes) = input::encode_key(key, input::Modifiers::default(), mode) {
                self.write(&bytes.repeat(lines.unsigned_abs() as usize));
            }
        } else {
            session.terminal_mut().scroll(lines);
            self.refresh();
        }
    }

    /// Brings the view back to the most recent output, as typing does.
    fn scroll_to_bottom(&mut self) {
        if let Some(session) = &mut self.session
            && session.terminal().scroll_offset() > 0
        {
            session.terminal_mut().scroll_to_bottom();
            self.refresh();
        }
    }

    /// Queues input for the shell. Returns whether it was accepted.
    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        match session.write(bytes) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("woop: failed to send input: {error}");
                false
            }
        }
    }

    fn refresh(&mut self) {
        if let Some(session) = &self.session {
            self.screen = session.terminal().screen();
            self.cache.clear();
        }
    }
}

/// Bridges the session's output notifications into the iced event loop.
fn output_listener() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async |mut output| {
        let (sender, mut wakeups) = mpsc::unbounded();
        if output.send(Message::ListenerReady(sender)).await.is_err() {
            return;
        }
        while wakeups.next().await.is_some() {
            if output.send(Message::Output).await.is_err() {
                break;
            }
        }
    })
}
