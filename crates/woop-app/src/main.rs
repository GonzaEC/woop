//! Woop application entry point: the window, its tabs and their panes
//! (ADR 0019).

mod editor;
mod gate;
mod history;
mod keys;
mod pane;
mod theme;
mod view;

use std::collections::HashMap;

use iced::futures::channel::mpsc::{self, UnboundedSender};
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, column, container, keyed_column, row, text};
use iced::{Background, Border, Element, Length, Subscription, Task};
use woop_term::{ShellCommand, TerminalSize, shell_integration};

use history::History;
use keys::{Direction, KeyAction, WorkspaceAction};
use pane::{Pane, PaneId};
use theme::Palette;
use view::Metrics;

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

/// Messages handled by the window.
#[derive(Debug, Clone)]
pub enum Message {
    /// The output listener is running; sessions can be started.
    ListenerReady(UnboundedSender<PaneId>),
    /// A pane's shell produced output.
    Output(PaneId),
    /// A message for one pane.
    Pane(PaneId, pane::Message),
    /// A pane of the shown tab was clicked.
    PaneClicked(pane_grid::Pane),
    /// The split between two panes was dragged.
    PaneResized(pane_grid::ResizeEvent),
    /// A tab was clicked.
    SelectTab(usize),
}

/// The layout of one tab's panes, and which one has the focus.
struct Tab {
    /// Stable identifier, so each tab keeps its own widget state: the pane
    /// grids of different tabs number their panes alike.
    id: u64,
    grid: pane_grid::State<PaneId>,
    focus: pane_grid::Pane,
}

struct Woop {
    /// Sends a pane's id when its session has output; set once the
    /// listener runs.
    output: Option<UnboundedSender<PaneId>>,
    panes: HashMap<PaneId, Pane>,
    tabs: Vec<Tab>,
    /// Index of the tab shown.
    active: usize,
    next_id: PaneId,
    /// Commands submitted from any pane's editor.
    history: History,
    palette: Palette,
    metrics: Metrics,
    /// A pane whose command is still running was asked to close; asking
    /// again closes it.
    pending_close: Option<PaneId>,
}

impl Woop {
    fn new() -> Self {
        Self {
            output: None,
            panes: HashMap::new(),
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            history: History::default(),
            palette: Palette::default(),
            metrics: Metrics::measure(view::default_font(), FONT_SIZE),
            pending_close: None,
        }
    }

    fn title(&self) -> String {
        match self.focused().and_then(|id| self.panes.get(&id)?.title()) {
            Some(title) => format!("{title} - {APP_NAME}"),
            None => APP_NAME.to_owned(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ListenerReady(sender) => {
                if self.output.is_none() {
                    self.output = Some(sender);
                    return self.new_tab();
                }
            }
            Message::Output(id) => {
                let Some(pane) = self.panes.get_mut(&id) else {
                    return Task::none();
                };
                let task = pane.process_output().map(move |m| Message::Pane(id, m));
                // A pane closes when its shell exits.
                if pane.is_closed() {
                    return Task::batch([task, self.close_pane(id)]);
                }
                return task;
            }
            Message::Pane(_, pane::Message::Key(KeyAction::Workspace(action))) => {
                return self.workspace(action);
            }
            Message::Pane(id, message) => {
                if matches!(
                    message,
                    pane::Message::Key(_) | pane::Message::EditorPress(_)
                ) {
                    self.pending_close = None;
                }
                let mut task = Task::none();
                if message.is_click() {
                    task = self.focus_pane(id);
                }
                // Keys handled in one batch with a focus change were read by
                // the pane that had the focus before; they must not reach a
                // shell the user is no longer looking at.
                if !accepts(self.focused(), id, &message) {
                    return task;
                }
                if let Some(pane) = self.panes.get_mut(&id) {
                    let update = pane
                        .update(message, &mut self.history)
                        .map(move |m| Message::Pane(id, m));
                    task = Task::batch([task, update]);
                }
                return task;
            }
            Message::PaneClicked(handle) => {
                if let Some(id) = self.tab().and_then(|tab| tab.grid.get(handle).copied()) {
                    return self.focus_pane(id);
                }
            }
            Message::PaneResized(event) => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.grid.resize(event.split, event.ratio);
                }
            }
            Message::SelectTab(index) => {
                if index < self.tabs.len() {
                    self.active = index;
                    return self.focus_current();
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let Some(tab) = self.tab() else {
            return container(text("")).into();
        };
        let several = tab.grid.len() > 1;
        let palette = &self.palette;
        let grid = PaneGrid::new(&tab.grid, |handle, &id, _maximized| {
            let focused = handle == tab.focus;
            let content = match self.panes.get(&id) {
                Some(pane) => pane
                    .view(palette, focused)
                    .map(move |m| Message::Pane(id, m)),
                None => text("").into(),
            };
            // With several panes, the focused one is outlined.
            let outline = if several && focused {
                palette.cursor
            } else {
                palette.separator
            };
            pane_grid::Content::new(content).style(move |_theme| container::Style {
                border: Border {
                    color: outline,
                    width: if several { 1.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            })
        })
        .on_click(Message::PaneClicked)
        .on_resize(8, Message::PaneResized)
        .spacing(2)
        .width(Length::Fill)
        .height(Length::Fill);

        let mut layout = column![];
        if self.tabs.len() > 1 {
            layout = layout.push(self.tab_bar());
        }
        if self.pending_close.is_some() {
            layout = layout.push(
                container(
                    text(close_prompt())
                        .font(self.metrics.font)
                        .size(self.metrics.font_size)
                        .color(palette.failure),
                )
                .padding(4),
            );
        }
        let background = palette.background;
        // Keyed by the tab, so switching tabs gives each its own widget
        // state (grid sizes, editor focus) instead of reusing another's.
        let grid = keyed_column([(tab.id, Element::from(grid))]).height(Length::Fill);
        container(layout.push(grid))
            .style(move |_theme| container::Style {
                background: Some(Background::Color(background)),
                ..container::Style::default()
            })
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(output_listener)
    }

    /// A row of buttons, one per tab, named after each tab's focused pane.
    fn tab_bar(&self) -> Element<'_, Message> {
        let palette = &self.palette;
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            let label = tab
                .grid
                .get(tab.focus)
                .and_then(|id| self.panes.get(id)?.title())
                .unwrap_or(APP_NAME);
            let (foreground, background) = if index == self.active {
                (palette.foreground, palette.selection)
            } else {
                (
                    theme::dim(palette.foreground, palette.background),
                    palette.background,
                )
            };
            // Tabs share the width and never wrap, so any number fits.
            button(
                text(tab_label(index, label))
                    .font(self.metrics.font)
                    .size(self.metrics.font_size)
                    .color(foreground)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .width(Length::Fill)
            .clip(true)
            .on_press(Message::SelectTab(index))
            .style(move |_theme, _status| button::Style {
                background: Some(Background::Color(background)),
                text_color: foreground,
                ..button::Style::default()
            })
            .into()
        });
        row(tabs).spacing(2).into()
    }

    fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// The pane with the keyboard focus.
    fn focused(&self) -> Option<PaneId> {
        let tab = self.tab()?;
        tab.grid.get(tab.focus).copied()
    }

    /// Gives the keyboard to the focused pane's editor, if it is active.
    fn focus_current(&self) -> Task<Message> {
        match self.focused() {
            Some(id) => self.panes[&id].focus().map(move |m| Message::Pane(id, m)),
            None => Task::none(),
        }
    }

    /// Moves the focus to pane `id` of the shown tab.
    fn focus_pane(&mut self, id: PaneId) -> Task<Message> {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return Task::none();
        };
        if let Some((&handle, _)) = tab.grid.iter().find(|&(_, &pane)| pane == id) {
            tab.focus = handle;
        }
        self.focus_current()
    }

    fn workspace(&mut self, action: WorkspaceAction) -> Task<Message> {
        if action != WorkspaceAction::ClosePane {
            self.pending_close = None;
        }
        match action {
            WorkspaceAction::NewTab => self.new_tab(),
            WorkspaceAction::ClosePane => {
                let Some(id) = self.focused() else {
                    return Task::none();
                };
                // A running command is only ended after a second request.
                if self.panes[&id].is_running() && self.pending_close != Some(id) {
                    self.pending_close = Some(id);
                    return Task::none();
                }
                self.close_pane(id)
            }
            WorkspaceAction::NextTab | WorkspaceAction::PreviousTab => {
                let count = self.tabs.len();
                if count > 1 {
                    self.active = if action == WorkspaceAction::NextTab {
                        (self.active + 1) % count
                    } else {
                        (self.active + count - 1) % count
                    };
                }
                self.focus_current()
            }
            WorkspaceAction::SplitRight => self.split(pane_grid::Axis::Vertical),
            WorkspaceAction::SplitDown => self.split(pane_grid::Axis::Horizontal),
            WorkspaceAction::Focus(direction) => {
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    return Task::none();
                };
                let direction = match direction {
                    Direction::Left => pane_grid::Direction::Left,
                    Direction::Right => pane_grid::Direction::Right,
                    Direction::Up => pane_grid::Direction::Up,
                    Direction::Down => pane_grid::Direction::Down,
                };
                if let Some(next) = tab.grid.adjacent(tab.focus, direction) {
                    tab.focus = next;
                }
                self.focus_current()
            }
        }
    }

    /// Starts a shell in a new pane, at the size of the focused pane.
    fn spawn_pane(&mut self) -> Option<PaneId> {
        let output = self.output.clone()?;
        let id = self.next_id;
        self.next_id += 1;
        let size = self
            .focused()
            .and_then(|focused| self.panes.get(&focused))
            .map_or(INITIAL_SIZE, Pane::size);
        let notify = move || {
            let _ = output.unbounded_send(id);
        };
        match shell_integration::spawn(ShellCommand::default_shell(), size, notify) {
            Ok(session) => {
                self.panes
                    .insert(id, Pane::new(id, session, size, self.metrics));
                Some(id)
            }
            Err(error) => {
                eprintln!("woop: failed to start the shell: {error}");
                None
            }
        }
    }

    fn new_tab(&mut self) -> Task<Message> {
        let Some(id) = self.spawn_pane() else {
            // Without any pane there is nothing to show.
            return if self.tabs.is_empty() {
                iced::exit()
            } else {
                Task::none()
            };
        };
        let (grid, focus) = pane_grid::State::new(id);
        // Pane ids are never reused, so the first pane's id also identifies
        // the tab.
        self.tabs.push(Tab { id, grid, focus });
        self.active = self.tabs.len() - 1;
        self.focus_current()
    }

    fn split(&mut self, axis: pane_grid::Axis) -> Task<Message> {
        let Some(id) = self.spawn_pane() else {
            return Task::none();
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return Task::none();
        };
        if let Some((handle, _)) = tab.grid.split(axis, tab.focus, id) {
            tab.focus = handle;
        }
        self.focus_current()
    }

    /// Closes pane `id`, ending its session. Its tab closes with its last
    /// pane, and the window with its last tab.
    fn close_pane(&mut self, id: PaneId) -> Task<Message> {
        self.pending_close = None;
        self.panes.remove(&id);
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.grid.iter().any(|(_, &pane)| pane == id))
        else {
            return Task::none();
        };
        let tab = &mut self.tabs[index];
        let handle = tab
            .grid
            .iter()
            .find(|&(_, &pane)| pane == id)
            .map(|(&handle, _)| handle);
        match handle.and_then(|handle| tab.grid.close(handle)) {
            Some((_, sibling)) => {
                if Some(tab.focus) == handle {
                    tab.focus = sibling;
                }
            }
            // The last pane of the tab.
            None => {
                self.tabs.remove(index);
                if self.tabs.is_empty() {
                    return iced::exit();
                }
                if self.active >= index && self.active > 0 {
                    self.active -= 1;
                }
            }
        }
        self.focus_current()
    }
}

/// Whether pane `id` may handle `message` while `focused` has the focus:
/// keyboard input only reaches the focused pane.
fn accepts(focused: Option<PaneId>, id: PaneId, message: &pane::Message) -> bool {
    !message.is_keyboard_input() || focused == Some(id)
}

/// Most characters of a title shown on a tab.
const TAB_LABEL_CHARS: usize = 24;

/// A short label for tab `index` (shown from 1) with the given title: the
/// last part of a path, such as a program or a directory, and at most
/// [`TAB_LABEL_CHARS`] characters of it.
fn tab_label(index: usize, title: &str) -> String {
    let name = title
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(title)
        .trim();
    let mut label: String = name.chars().take(TAB_LABEL_CHARS).collect();
    if name.chars().count() > TAB_LABEL_CHARS {
        label.pop();
        label.push('…');
    }
    format!("{} {label}", index + 1)
}

/// Asks for a second request before ending a running command.
fn close_prompt() -> &'static str {
    match keys::Shortcuts::platform() {
        keys::Shortcuts::MacOs => "A command is still running: press Cmd+W again to close it",
        keys::Shortcuts::Windows | keys::Shortcuts::Unix => {
            "A command is still running: press Ctrl+Shift+W again to close it"
        }
    }
}

/// Bridges the sessions' output notifications into the iced event loop.
fn output_listener() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async |mut output| {
        let (sender, mut wakeups) = mpsc::unbounded();
        if output.send(Message::ListenerReady(sender)).await.is_err() {
            return;
        }
        while let Some(id) = wakeups.next().await {
            if output.send(Message::Output(id)).await.is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_input_only_reaches_the_focused_pane() {
        use keys::KeyAction;
        let typed = pane::Message::Key(KeyAction::Input(b"rm -rf build\r".to_vec()));
        assert!(accepts(Some(1), 1, &typed));
        // Read by pane 1 just before the focus moved to pane 2.
        assert!(!accepts(Some(2), 1, &typed));
        assert!(!accepts(None, 1, &typed));
        assert!(!accepts(
            Some(2),
            1,
            &pane::Message::Pasted(Some("text".into()))
        ));
        assert!(!accepts(Some(2), 1, &pane::Message::Key(KeyAction::Paste)));
        // Clicks and output-driven updates are not keyboard input: a click
        // focuses its pane first.
        assert!(accepts(Some(2), 1, &pane::Message::PasteClicked));
        assert!(accepts(Some(2), 1, &pane::Message::Scroll(3)));
        assert!(accepts(
            Some(2),
            1,
            &pane::Message::Resized(TerminalSize::new(10, 40))
        ));
    }

    #[test]
    fn tab_labels_are_short() {
        assert_eq!(
            tab_label(
                0,
                r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe"
            ),
            "1 powershell.exe"
        );
        assert_eq!(tab_label(1, "user@host: ~/src/woop/"), "2 woop");
        assert_eq!(tab_label(2, "vim"), "3 vim");
        assert_eq!(tab_label(3, "/"), "4 /");
        assert_eq!(
            tab_label(4, "a-very-long-program-name-that-goes-on"),
            "5 a-very-long-program-nam…"
        );
    }
}
