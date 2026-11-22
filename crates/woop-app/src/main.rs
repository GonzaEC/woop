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

use gate::Gate;
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
    /// The command editor edited its text or moved its cursor.
    EditorAction(text_editor::Action),
    /// A key of the command editor with a meaning of its own.
    EditorKey(EditorKey),
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
    /// The shell reported a prompt at least once, so the editor's panel is
    /// shown from then on.
    integration: bool,
    /// Which prompt the editor takes the keys for.
    gate: Gate,
    /// Lines of text the editor's panel has room for.
    panel_lines: usize,
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
                    // Kept as is in the editor; control characters are
                    // removed when the command is sent.
                    self.edit(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(text),
                    )));
                } else if let Some(session) = &self.session {
                    let bytes = input::encode_paste(&text, session.terminal().mode());
                    self.write(&bytes);
                }
            }
            Message::Pasted(None) => {}
            Message::EditorAction(action) => {
                if self.editor_active() {
                    self.edit(action);
                } else if let text_editor::Action::Edit(text_editor::Edit::Insert(ch)) = action {
                    // Typed right after Enter, before the panel was redrawn
                    // as inactive: the key belongs to the shell now, in
                    // order with the ones after it.
                    let mut buffer = [0; 4];
                    self.write(ch.encode_utf8(&mut buffer).as_bytes());
                }
            }
            Message::EditorKey(key) => {
                if self.editor_active() {
                    return self.editor_key(key);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let terminal = self.session.as_ref().map(Session::terminal);
        let active = self.editor_active();
        let keys = keys::KeyContext {
            mode: terminal.map(Terminal::mode).unwrap_or_default(),
            has_selection: terminal.is_some_and(Terminal::has_selection),
            shortcuts: keys::Shortcuts::platform(),
            editor_active: active,
        };
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
            self.history.stop_walking();
            self.gate.edited();
        }
        self.editor.perform(action);
        self.panel_lines = editor::lines_for(&self.editor);
    }

    /// Replaces the editor's text, with the cursor at its end.
    fn set_editor_text(&mut self, text: &str) {
        self.editor = Content::with_text(text);
        self.editor
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        self.panel_lines = editor::lines_for(&self.editor);
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
                    self.gate.consume();
                    self.editor = Content::new();
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
                        self.gate.consume();
                        self.editor = Content::new();
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
            // Handled by the editor widget itself.
            EditorKey::Edit
            | EditorKey::Ignore
            | EditorKey::Newline
            | EditorKey::Copy
            | EditorKey::Paste => {}
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
