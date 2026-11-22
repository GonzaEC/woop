//! Woop application entry point.

mod keys;
mod theme;
mod view;

use iced::futures::channel::mpsc::{self, UnboundedSender};
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::widget::canvas;
use iced::{Element, Length, Subscription, Task, clipboard};
use woop_term::{
    Screen, SelectionKind, SelectionPoint, Session, ShellCommand, Terminal, TerminalEvent,
    TerminalSize, input,
};

use keys::KeyAction;
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
    /// A key was pressed.
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
}

struct Woop {
    session: Option<Session>,
    screen: Screen,
    size: TerminalSize,
    palette: Palette,
    metrics: Metrics,
    cache: canvas::Cache,
    title: Option<String>,
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
        }
    }

    fn title(&self) -> String {
        match &self.title {
            Some(title) if !title.is_empty() => format!("{title} - {APP_NAME}"),
            _ => APP_NAME.to_owned(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ListenerReady(sender) => self.start_session(sender),
            Message::Output => return self.process_output(),
            Message::Key(KeyAction::Input(bytes)) => {
                if let Some(session) = &mut self.session {
                    if session.terminal().scroll_offset() > 0 {
                        session.terminal_mut().scroll_to_bottom();
                        self.refresh();
                    }
                    self.write(&bytes);
                }
            }
            Message::Key(KeyAction::Paste) => return clipboard::read().map(Message::Pasted),
            Message::Key(KeyAction::Copy) => {
                // The selection is cleared once copied, so that a second
                // Ctrl+C on Windows interrupts the shell as usual.
                if let Some(session) = &mut self.session
                    && let Some(text) = session.terminal().selection_text()
                {
                    session.terminal_mut().clear_selection();
                    self.refresh();
                    return clipboard::write(text);
                }
            }
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
                if let Some(session) = &self.session {
                    let bytes = input::encode_paste(&text, session.terminal().mode());
                    self.write(&bytes);
                }
            }
            Message::Pasted(None) => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let terminal = self.session.as_ref().map(Session::terminal);
        let keys = keys::KeyContext {
            mode: terminal.map(Terminal::mode).unwrap_or_default(),
            has_selection: terminal.is_some_and(Terminal::has_selection),
            shortcuts: keys::Shortcuts::platform(),
        };
        canvas(TerminalView {
            screen: &self.screen,
            palette: &self.palette,
            metrics: self.metrics,
            keys,
            cache: &self.cache,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(output_listener)
    }

    fn start_session(&mut self, sender: UnboundedSender<()>) {
        if self.session.is_some() {
            return;
        }
        let notify = move || {
            let _ = sender.unbounded_send(());
        };
        match Session::spawn_with_notifier(&ShellCommand::default_shell(), self.size, notify) {
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

        let mut tasks = Vec::new();
        for event in events {
            match event {
                TerminalEvent::Title(title) => self.title = Some(title),
                TerminalEvent::ResetTitle => self.title = None,
                TerminalEvent::ClipboardStore(text) => tasks.push(clipboard::write(text)),
                TerminalEvent::Bell | TerminalEvent::Reply(_) => {}
            }
        }
        self.refresh();
        Task::batch(tasks)
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

    fn write(&mut self, bytes: &[u8]) {
        if let Some(session) = &mut self.session
            && let Err(error) = session.write(bytes)
        {
            eprintln!("woop: failed to send input: {error}");
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
