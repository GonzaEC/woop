//! Woop application entry point: the window, its tabs and their panes
//! (ADR 0019).

mod bindings;
mod editor;
mod gate;
mod history;
mod keys;
mod palette;
mod pane;
mod theme;
mod view;

use std::collections::HashMap;
use std::sync::Arc;

use iced::futures::channel::mpsc::{self, UnboundedSender};
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, column, container, keyed_column, mouse_area, row, stack, text};
use iced::{Background, Border, Element, Length, Subscription, Task, keyboard};
use woop_term::{ShellCommand, TerminalSize, shell_integration};

use bindings::Bindings;
use history::History;
use keys::{Direction, KeyAction, WorkspaceAction};
use palette::{Command, Outcome, Palette as CommandPalette};
use pane::{Pane, PaneCommand, PaneId};
use theme::Palette;
use view::Metrics;
use woop_config::Config;

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
    /// The configuration file changed on disk.
    ConfigChanged,
    /// A key was pressed while the command palette is open.
    PaletteKey(keyboard::Key, keyboard::Modifiers, Option<String>),
    /// A command was clicked in the command palette.
    PaletteRun(Command),
    /// The command palette was dismissed with a click outside it.
    PaletteClose,
}

/// Most different font family names kept during one run (ADR 0020).
const MAX_FONT_NAMES: usize = 32;

/// How often the configuration file is checked for changes (ADR 0020).
const CONFIG_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

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
    /// The keyboard shortcuts in effect (ADR 0021).
    bindings: Arc<Bindings>,
    metrics: Metrics,
    /// A pane whose command is still running was asked to close; asking
    /// again closes it.
    pending_close: Option<PaneId>,
    /// The settings in use (ADR 0020).
    config: Config,
    /// Why the configuration file could not be used, shown until fixed.
    config_error: Option<String>,
    /// Why the configured shell could not start, shown while the default
    /// shell is used instead.
    shell_error: Option<String>,
    /// Font family names given to iced, which keeps them for the whole
    /// program; each distinct name is kept once.
    font_names: Vec<&'static str>,
    /// The command palette, while it is open. It then takes every key.
    command_palette: Option<CommandPalette>,
    /// The pane focused when the palette opened: its commands act on that
    /// pane only.
    palette_target: Option<PaneId>,
}

impl Woop {
    fn new() -> Self {
        let mut woop = Self {
            output: None,
            panes: HashMap::new(),
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            history: History::default(),
            palette: Palette::default(),
            bindings: Arc::new(Bindings::default()),
            metrics: Metrics::measure(view::default_font(), woop_config::DEFAULT_FONT_SIZE),
            pending_close: None,
            config: Config::default(),
            config_error: None,
            shell_error: None,
            font_names: Vec::new(),
            command_palette: None,
            palette_target: None,
        };
        woop.reload_config();
        woop
    }

    /// Reads the configuration file again. A file that cannot be used
    /// leaves the current settings in place and is reported; a missing file
    /// means the defaults.
    fn reload_config(&mut self) {
        let Some(path) = woop_config::path() else {
            return;
        };
        match woop_config::load(&path) {
            Ok(config) => {
                self.config_error = None;
                self.apply_config(config.unwrap_or_default());
            }
            Err(error) => self.config_error = Some(error.to_string()),
        }
    }

    /// Applies settings: colours and the font at once to every pane, the
    /// shell to panes opened from now on.
    fn apply_config(&mut self, config: Config) {
        if config == self.config {
            return;
        }
        self.palette = Palette::from(&config.colors);
        self.bindings = Arc::new(Bindings::new(keys::Shortcuts::platform(), &config.keys));
        let font = config
            .font
            .family
            .as_deref()
            .and_then(|family| self.font_name(family))
            .map_or_else(view::default_font, iced::Font::with_name);
        self.metrics = Metrics::measure(font, config.font.size);
        for pane in self.panes.values_mut() {
            pane.restyle(self.metrics, Arc::clone(&self.bindings));
        }
        if config.shell != self.config.shell {
            // Checked again when the next pane starts.
            self.shell_error = None;
        }
        self.config = config;
    }

    /// The `'static` name iced needs for a font family, kept once per
    /// distinct name. After [`MAX_FONT_NAMES`] different names, new ones
    /// are not kept and the default font is used, so memory stays bounded.
    fn font_name(&mut self, family: &str) -> Option<&'static str> {
        if let Some(&name) = self.font_names.iter().find(|&&name| name == family) {
            return Some(name);
        }
        if self.font_names.len() >= MAX_FONT_NAMES {
            return None;
        }
        let name: &'static str = Box::leak(family.to_owned().into_boxed_str());
        self.font_names.push(name);
        Some(name)
    }

    fn title(&self) -> String {
        match self.focused().and_then(|id| self.panes.get(&id)?.title()) {
            Some(title) => format!("{title} - {APP_NAME}"),
            None => APP_NAME.to_owned(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        // Whatever changed the shown tab, its panes are drawn next, from an
        // up-to-date snapshot.
        if let Some(tab) = self.tabs.get(self.active) {
            for (_, id) in tab.grid.iter() {
                if let Some(pane) = self.panes.get_mut(id) {
                    pane.show();
                }
            }
        }
        task
    }

    /// Whether pane `id` is in the shown tab.
    fn is_shown(&self, id: PaneId) -> bool {
        self.tab()
            .is_some_and(|tab| tab.grid.iter().any(|(_, &pane)| pane == id))
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ListenerReady(sender) => {
                if self.output.is_none() {
                    self.output = Some(sender);
                    return self.new_tab();
                }
            }
            Message::Output(id) => {
                let shown = self.is_shown(id);
                let Some(pane) = self.panes.get_mut(&id) else {
                    return Task::none();
                };
                let task = pane
                    .process_output(shown)
                    .map(move |m| Message::Pane(id, m));
                // A pane closes when its shell exits.
                if pane.is_closed() {
                    return Task::batch([task, self.close_pane(id)]);
                }
                return task;
            }
            Message::Pane(_, pane::Message::Key(KeyAction::Workspace(action))) => {
                // Read by a pane just before the palette opened.
                if self.command_palette.is_none() {
                    return self.workspace(action);
                }
            }
            // A palette command given a shortcut in the configuration: it
            // acts on the focused pane that read the key.
            Message::Pane(id, pane::Message::Key(KeyAction::Command(command))) => {
                // Like any other key, it withdraws a pending close request.
                self.pending_close = None;
                if self.command_palette.is_none() && self.focused() == Some(id) {
                    self.palette_target = Some(id);
                    return self.run_command(command);
                }
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
                // shell the user is no longer looking at. While the command
                // palette is open it takes every key.
                if !accepts(self.focused(), self.command_palette.is_some(), id, &message) {
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
                    // The close request was for a pane no longer shown.
                    self.pending_close = None;
                    self.active = index;
                    return self.focus_current();
                }
            }
            Message::ConfigChanged => self.reload_config(),
            Message::PaletteKey(key, modifiers, text) => {
                // The palette's own shortcut closes it again.
                let toggle = keys::key_press(&key, modifiers, None, &self.idle_key_context())
                    == Some(KeyAction::Workspace(WorkspaceAction::CommandPalette));
                let Some(palette) = &mut self.command_palette else {
                    return Task::none();
                };
                let outcome = if toggle {
                    Outcome::Close
                } else {
                    palette.key(&key, modifiers, text.as_deref())
                };
                match outcome {
                    Outcome::Stay => {}
                    Outcome::Close => return self.close_palette(),
                    Outcome::Run(command) => return self.run_command(command),
                }
            }
            Message::PaletteRun(command) => {
                if self.command_palette.is_some() {
                    return self.run_command(command);
                }
            }
            Message::PaletteClose => return self.close_palette(),
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
                    .view(palette, focused && self.command_palette.is_none())
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
        let notices = [
            self.pending_close.map(|_| {
                close_prompt(palette::shortcut(
                    Command::ClosePane,
                    keys::Shortcuts::platform(),
                    &self.bindings,
                ))
            }),
            self.config_error.as_ref().map(|error| {
                format!("Configuration not applied, the previous settings stay: {error}")
            }),
            self.shell_error.as_ref().map(|error| {
                format!("The configured shell could not start, the default shell is used: {error}")
            }),
        ];
        for notice in notices.into_iter().flatten() {
            layout = layout.push(
                container(
                    text(notice)
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
        let window = container(layout.push(grid)).style(move |_theme| container::Style {
            background: Some(Background::Color(background)),
            ..container::Style::default()
        });
        // The same structure whether the palette is open or not, so opening
        // it does not rebuild the panes' widget state.
        let mut layers = stack![window];
        if let Some(command_palette) = &self.command_palette {
            layers = layers.push(self.palette_view(command_palette));
        }
        layers.into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            Subscription::run(output_listener),
            Subscription::run(config_listener),
        ];
        // Keys reach the command palette only while it is open; the panes
        // take none meanwhile, so none is captured before it.
        if self.command_palette.is_some() {
            subscriptions.push(keyboard::listen().filter_map(palette_key));
        }
        Subscription::batch(subscriptions)
    }

    /// Key meanings with nothing selected and no editor, for shortcuts that
    /// do not depend on a pane.
    fn idle_key_context(&self) -> keys::KeyContext {
        keys::KeyContext {
            mode: woop_term::TerminalMode::default(),
            has_selection: false,
            shortcuts: keys::Shortcuts::platform(),
            editor_active: false,
            bindings: Arc::clone(&self.bindings),
        }
    }

    fn close_palette(&mut self) -> Task<Message> {
        self.command_palette = None;
        self.palette_target = None;
        self.focus_current()
    }

    /// Runs a command chosen in the command palette, which closes.
    fn run_command(&mut self, command: Command) -> Task<Message> {
        let target = self.palette_target;
        let task = self.close_palette();
        // A command that acts on a pane runs only on the one the palette
        // was opened for: if it closed in the meantime, another pane now has
        // the focus and the user did not choose it.
        let on_target = self
            .focused()
            .is_some_and(|focused| still_target(target, focused));
        if acts_on_pane(command) && !on_target {
            return task;
        }
        // Kept while the command runs, for the pane commands.
        self.palette_target = target;
        let run = match command {
            Command::CommandPalette => self.workspace(WorkspaceAction::CommandPalette),
            Command::NewTab => self.workspace(WorkspaceAction::NewTab),
            Command::ClosePane => self.workspace(WorkspaceAction::ClosePane),
            Command::NextTab => self.workspace(WorkspaceAction::NextTab),
            Command::PreviousTab => self.workspace(WorkspaceAction::PreviousTab),
            Command::SplitRight => self.workspace(WorkspaceAction::SplitRight),
            Command::SplitDown => self.workspace(WorkspaceAction::SplitDown),
            Command::FocusLeft => self.workspace(WorkspaceAction::Focus(Direction::Left)),
            Command::FocusRight => self.workspace(WorkspaceAction::Focus(Direction::Right)),
            Command::FocusUp => self.workspace(WorkspaceAction::Focus(Direction::Up)),
            Command::FocusDown => self.workspace(WorkspaceAction::Focus(Direction::Down)),
            Command::CopyLastCommandLine => {
                self.send_to_focused(pane::Message::Command(PaneCommand::CopyLastCommand))
            }
            Command::CopyLastOutput => {
                self.send_to_focused(pane::Message::Command(PaneCommand::CopyLastOutput))
            }
            Command::ToggleLastOutput => {
                self.send_to_focused(pane::Message::Command(PaneCommand::ToggleLastOutput))
            }
            Command::ScrollToBottom => {
                self.send_to_focused(pane::Message::Command(PaneCommand::ScrollToBottom))
            }
            Command::PreviousBlock => {
                self.send_to_focused(pane::Message::Key(KeyAction::JumpBlock(-1)))
            }
            Command::NextBlock => self.send_to_focused(pane::Message::Key(KeyAction::JumpBlock(1))),
            Command::CopySelection => self.send_to_focused(pane::Message::Key(KeyAction::Copy)),
            Command::Paste => self.send_to_focused(pane::Message::Key(KeyAction::Paste)),
            Command::CopyConfigPath => match woop_config::path() {
                Some(path) => iced::clipboard::write(path.display().to_string()),
                None => Task::none(),
            },
            Command::ReloadConfig => {
                self.reload_config();
                Task::none()
            }
        };
        self.palette_target = None;
        Task::batch([task, run])
    }

    /// Sends a message to the focused pane.
    fn send_to_focused(&mut self, message: pane::Message) -> Task<Message> {
        // Only to the pane the palette was opened for: if it closed in the
        // meantime, another pane now has the focus and the user did not
        // choose it.
        match self.focused() {
            Some(id) if still_target(self.palette_target, id) => {
                self.update(Message::Pane(id, message))
            }
            _ => Task::none(),
        }
    }

    /// The command palette, drawn over the panes. A click outside it closes
    /// it.
    fn palette_view<'a>(&'a self, command_palette: &'a CommandPalette) -> Element<'a, Message> {
        /// Commands shown at once; the list follows the chosen one.
        const VISIBLE: usize = 12;
        let palette = &self.palette;
        let metrics = self.metrics;
        let muted = theme::dim(palette.foreground, palette.background);
        let shortcuts = keys::Shortcuts::platform();
        let matches = command_palette.matches();
        let selected = command_palette.selected();
        let first = selected.saturating_sub(VISIBLE - 1);

        let line = |label: String, color: iced::Color| {
            text(label)
                .font(metrics.font)
                .size(metrics.font_size)
                .color(color)
                .wrapping(iced::widget::text::Wrapping::None)
        };
        let search = if command_palette.query().is_empty() {
            line("Type a command".to_owned(), muted)
        } else {
            line(
                format!("{}\u{2502}", command_palette.query()),
                palette.foreground,
            )
        };
        let items =
            matches
                .iter()
                .enumerate()
                .skip(first)
                .take(VISIBLE)
                .map(|(index, &command)| {
                    let background = if index == selected {
                        palette.selection
                    } else {
                        palette.background
                    };
                    let shortcut =
                        palette::shortcut(command, shortcuts, &self.bindings).unwrap_or_default();
                    button(
                        row![
                            line(palette::label(command).to_owned(), palette.foreground)
                                .width(Length::Fill),
                            line(shortcut, muted),
                        ]
                        .spacing(16),
                    )
                    .on_press(Message::PaletteRun(command))
                    .width(Length::Fill)
                    .style(move |_theme, _status| button::Style {
                        background: Some(Background::Color(background)),
                        ..button::Style::default()
                    })
                    .into()
                });
        let mut list = column![search].spacing(4);
        if matches.is_empty() {
            list = list.push(line("No command matches".to_owned(), muted));
        }
        let list = list.extend(items);

        let (background, outline) = (palette.background, palette.cursor);
        let panel = container(list)
            .width(Length::Fixed(560.0))
            .padding(8)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    color: outline,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            });
        let overlay = container(iced::widget::opaque(panel))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .padding(iced::Padding {
                top: 48.0,
                ..iced::Padding::ZERO
            });
        iced::widget::opaque(mouse_area(overlay).on_press(Message::PaletteClose))
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
        if withdraws_close(action) {
            self.pending_close = None;
        }
        match action {
            WorkspaceAction::NewTab => self.new_tab(),
            WorkspaceAction::CommandPalette => {
                self.command_palette = Some(CommandPalette::default());
                self.palette_target = self.focused();
                Task::none()
            }
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
        let focused = self.focused().and_then(|focused| self.panes.get(&focused));
        let size = focused.map_or(INITIAL_SIZE, Pane::size);
        // Where the focused pane's shell is, as it reported in a signed mark
        // (ADR 0022); otherwise the default directory.
        let directory = focused.and_then(Pane::current_directory);
        let notify = move || {
            let _ = output.unbounded_send(id);
        };
        let configured = self.config.shell.as_ref().map(|shell| {
            shell
                .args
                .iter()
                .fold(ShellCommand::new(&shell.program), |command, arg| {
                    command.arg(arg)
                })
        });
        let (started, shell_error) = start_shell(configured, |command| {
            let command = match &directory {
                Some(directory) => command.cwd(directory),
                None => command,
            };
            shell_integration::spawn(command, size, notify.clone())
        });
        if self.config.shell.is_some() {
            self.shell_error = shell_error;
        }
        match started {
            Ok(session) => {
                self.panes.insert(
                    id,
                    Pane::new(id, session, size, self.metrics, Arc::clone(&self.bindings)),
                );
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
                self.active = active_after_closing(self.active, index);
            }
        }
        self.focus_current()
    }
}

/// Starts the configured shell with `start`, or the default shell when none
/// is configured or the configured one cannot start, so that a mistake in
/// the configuration never leaves the window without a pane. Also returns
/// why the configured shell could not start.
fn start_shell<S, E: std::fmt::Display>(
    configured: Option<ShellCommand>,
    mut start: impl FnMut(ShellCommand) -> Result<S, E>,
) -> (Result<S, E>, Option<String>) {
    let Some(command) = configured else {
        return (start(ShellCommand::default_shell()), None);
    };
    match start(command) {
        Ok(session) => (Ok(session), None),
        Err(error) => (
            start(ShellCommand::default_shell()),
            Some(error.to_string()),
        ),
    }
}

/// The index of the shown tab after tab `closed` is removed, when tab
/// `active` was shown: the same tab if another closed, the one before it if
/// it closed itself (or the new first tab).
fn active_after_closing(active: usize, closed: usize) -> usize {
    if active >= closed && active > 0 {
        active - 1
    } else {
        active
    }
}

/// Whether `action` withdraws a request to close a pane with a running
/// command. Opening the palette does not, so that "Close pane" can be chosen
/// in it again to confirm.
fn withdraws_close(action: WorkspaceAction) -> bool {
    !matches!(
        action,
        WorkspaceAction::ClosePane | WorkspaceAction::CommandPalette
    )
}

/// Whether a palette command acts on the focused pane, and so may run only
/// on the pane the palette was opened for.
fn acts_on_pane(command: Command) -> bool {
    match command {
        Command::CommandPalette
        | Command::NewTab
        | Command::NextTab
        | Command::PreviousTab
        | Command::CopyConfigPath
        | Command::ReloadConfig => false,
        Command::ClosePane
        | Command::SplitRight
        | Command::SplitDown
        | Command::FocusLeft
        | Command::FocusRight
        | Command::FocusUp
        | Command::FocusDown
        | Command::CopyLastCommandLine
        | Command::CopyLastOutput
        | Command::ToggleLastOutput
        | Command::ScrollToBottom
        | Command::PreviousBlock
        | Command::NextBlock
        | Command::CopySelection
        | Command::Paste => true,
    }
}

/// Whether a command from the palette may act on the focused pane `id`: it
/// must be the pane focused when the palette opened.
fn still_target(target: Option<PaneId>, id: PaneId) -> bool {
    target == Some(id)
}

/// Whether pane `id` may handle `message` while `focused` has the focus:
/// keyboard input only reaches the focused pane, and none while the command
/// palette is open.
fn accepts(
    focused: Option<PaneId>,
    palette_open: bool,
    id: PaneId,
    message: &pane::Message,
) -> bool {
    !message.is_keyboard_input() || (focused == Some(id) && !palette_open)
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

/// Asks for a second request before ending a running command, naming the
/// close shortcut in effect, if it has one.
fn close_prompt(shortcut: Option<String>) -> String {
    match shortcut {
        Some(shortcut) => {
            format!("A command is still running: press {shortcut} again to close it")
        }
        None => "A command is still running: choose Close pane again to close it".to_owned(),
    }
}

/// Turns a key press into a message for the open command palette.
fn palette_key(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        } => Some(Message::PaletteKey(
            key,
            modifiers,
            text.map(|text| text.to_string()),
        )),
        _ => None,
    }
}

/// Reports changes to the configuration file, checked every
/// [`CONFIG_CHECK_INTERVAL`] by a background thread.
fn config_listener() -> impl Stream<Item = Message> {
    iced::stream::channel(1, async |mut output| {
        let Some(path) = woop_config::path() else {
            return;
        };
        let (sender, mut changes) = mpsc::unbounded();
        woop_config::watch(path, CONFIG_CHECK_INTERVAL, move || {
            let _ = sender.unbounded_send(());
        });
        while changes.next().await.is_some() {
            if output.send(Message::ConfigChanged).await.is_err() {
                break;
            }
        }
    })
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
        assert!(accepts(Some(1), false, 1, &typed));
        // Read by pane 1 just before the focus moved to pane 2.
        assert!(!accepts(Some(2), false, 1, &typed));
        assert!(!accepts(None, false, 1, &typed));
        assert!(!accepts(
            Some(2),
            false,
            1,
            &pane::Message::Pasted(Some("text".into()))
        ));
        assert!(!accepts(
            Some(2),
            false,
            1,
            &pane::Message::Key(KeyAction::Paste)
        ));
        // Clicks and output-driven updates are not keyboard input: a click
        // focuses its pane first.
        assert!(accepts(Some(2), false, 1, &pane::Message::PasteClicked));
        assert!(accepts(Some(2), false, 1, &pane::Message::Scroll(3)));
        assert!(accepts(
            Some(2),
            false,
            1,
            &pane::Message::Resized(TerminalSize::new(10, 40))
        ));
        // The open command palette takes every key, even from the focused
        // pane, while output still reaches it.
        assert!(!accepts(Some(1), true, 1, &typed));
        assert!(accepts(Some(1), true, 1, &pane::Message::Scroll(3)));
    }

    #[test]
    fn palette_commands_act_only_on_the_pane_they_were_opened_for() {
        assert!(still_target(Some(1), 1));
        // Pane 1 closed while the palette was open; pane 2 got the focus.
        assert!(!still_target(Some(1), 2));
        assert!(!still_target(None, 2));
    }

    #[test]
    fn workspace_commands_from_the_palette_need_their_pane() {
        // Closing, splitting and moving the focus act on a pane: they must
        // not reach a sibling that got the focus when the target closed.
        for command in [
            Command::ClosePane,
            Command::SplitRight,
            Command::SplitDown,
            Command::FocusLeft,
            Command::FocusDown,
            Command::Paste,
        ] {
            assert!(acts_on_pane(command), "{command:?}");
        }
        for command in [Command::NewTab, Command::NextTab, Command::ReloadConfig] {
            assert!(!acts_on_pane(command), "{command:?}");
        }
    }

    #[test]
    fn opening_the_palette_keeps_a_close_request() {
        // Otherwise "Close pane" chosen in the palette asks again forever.
        assert!(!withdraws_close(WorkspaceAction::CommandPalette));
        assert!(!withdraws_close(WorkspaceAction::ClosePane));
        assert!(withdraws_close(WorkspaceAction::NewTab));
        assert!(withdraws_close(WorkspaceAction::SplitRight));
        assert!(withdraws_close(WorkspaceAction::Focus(Direction::Left)));
    }

    #[test]
    fn the_close_request_names_the_shortcut_in_effect() {
        assert_eq!(
            close_prompt(Some("Ctrl+Shift+Q".to_owned())),
            "A command is still running: press Ctrl+Shift+Q again to close it"
        );
        // With the shortcut removed, the palette is the way to confirm.
        assert!(close_prompt(None).contains("choose Close pane again"));
    }

    #[test]
    fn a_shell_that_cannot_start_falls_back_to_the_default() {
        let configured = ShellCommand::new("no-such-shell");
        let mut tried = Vec::new();
        let (started, error) = start_shell(Some(configured.clone()), |command| {
            tried.push(command.clone());
            if command == ShellCommand::default_shell() {
                Ok("session")
            } else {
                Err("not found")
            }
        });
        assert_eq!(started, Ok("session"));
        assert_eq!(error.as_deref(), Some("not found"));
        assert_eq!(tried, [configured, ShellCommand::default_shell()]);

        // A shell that starts is used, and nothing is reported.
        let (started, error) = start_shell(Some(ShellCommand::new("sh")), |_| Ok::<_, &str>(1));
        assert_eq!((started, error), (Ok(1), None));
        let (_, error) = start_shell(None, |_| Err::<(), _>("broken"));
        assert_eq!(error, None, "only the configured shell is reported");
    }

    #[test]
    fn closing_a_tab_keeps_the_shown_one() {
        // Tabs 0..=3, tab 2 shown.
        assert_eq!(active_after_closing(2, 0), 1, "one before it closed");
        assert_eq!(active_after_closing(2, 3), 2, "one after it closed");
        assert_eq!(active_after_closing(2, 2), 1, "it closed: the previous one");
        assert_eq!(
            active_after_closing(0, 0),
            0,
            "the first closed: the new first"
        );
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
