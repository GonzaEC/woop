//! One terminal of the window: a shell session, its grid and its command
//! editor (ADR 0019).

use std::sync::Arc;

use iced::widget::text_editor::{self, Content};
use iced::widget::{canvas, column, container, stack};
use iced::{Element, Length, Task, clipboard};
use woop_term::{
    Screen, SelectionKind, SelectionPoint, Session, Terminal, TerminalEvent, TerminalSize, input,
};

use woop_term::shell_integration::Shell;

use crate::bindings::Bindings;
use woop_config::spec::Arg;

use crate::completion;
use crate::editor;
use crate::gate::{self, Consumed, Gate};
use crate::history::History;
use crate::keys::{self, EditorKey, KeyAction};
use crate::lookup;
use crate::specs::{self, Specs};
use crate::theme::Palette;
use crate::view::{Metrics, TerminalView};

/// After this long, a listing opens the completion list instead of
/// inserting its only match (ADR 0023).
const LATE_LISTING: std::time::Duration = std::time::Duration::from_millis(300);

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
    /// A command chosen in the command palette.
    Command(PaneCommand),
    /// A directory listing for completion request `0` arrived (ADR 0023).
    Listed(u64, Arc<[completion::Entry]>),
    /// An entry of the completion list was clicked.
    CompletionClicked(usize),
}

/// Commands from the command palette that act on one pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCommand {
    /// Copy the last finished command.
    CopyLastCommand,
    /// Copy the output of the last finished command.
    CopyLastOutput,
    /// Collapse or expand the output of the last finished command.
    ToggleLastOutput,
    /// Move the view back to the latest output.
    ScrollToBottom,
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
                | Self::CompletionClicked(_)
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
    /// The keyboard shortcuts in effect (ADR 0021).
    bindings: Arc<Bindings>,
    /// Output arrived while the pane was not shown: `screen` is out of date
    /// until it is shown again.
    stale: bool,
    /// The shell the pane runs, if Woop has an integration for it: it
    /// decides quoting and which history file applies.
    shell: Option<Shell>,
    /// The completion list, while it is open (ADR 0023).
    completion: Option<completion::List>,
    /// A completion waiting for a directory listing.
    pending: Option<Pending>,
    /// Identifies completion requests, so that late listings are dropped.
    requests: u64,
    /// A directory listing is running. Only one runs at a time, so a hung
    /// file system blocks one thread, not one per Tab.
    listing: bool,
    /// The command specifications in effect (ADR 0024).
    specs: Arc<Specs>,
}

/// A completion waiting for its directory listing.
struct Pending {
    id: u64,
    /// The editor's text and cursor when Tab was pressed; the listing is
    /// dropped if they changed meanwhile.
    text: String,
    cursor: text_editor::Position,
    path: completion::PathRequest,
    /// Completions known without reading anything (subcommands, options).
    known: Vec<completion::Candidate>,
    /// What the listing is for.
    want: Want,
    history: Vec<completion::Candidate>,
    /// When Tab was pressed.
    asked: std::time::Instant,
}

/// What a listing is turned into.
#[derive(Debug, Clone, Copy)]
enum Want {
    /// Paths, possibly only directories.
    Paths { only_directories: bool },
    /// Plain names (git refs, SSH hosts).
    Names,
}

/// What is read off the interface thread for a completion.
#[derive(Debug)]
enum Lookup {
    /// A directory's entries.
    Directory(std::path::PathBuf),
    /// Names of the git repository containing this directory.
    Git(std::path::PathBuf, lookup::GitNames),
    /// Hosts from the SSH configuration under this home directory.
    SshHosts(std::path::PathBuf),
}

impl Pane {
    /// Wraps a session that was just started, at `size`.
    pub fn new(
        id: PaneId,
        session: Session,
        size: TerminalSize,
        metrics: Metrics,
        bindings: Arc<Bindings>,
        shell: Option<Shell>,
        specs: Arc<Specs>,
    ) -> Self {
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
            bindings,
            stale: false,
            shell,
            completion: None,
            pending: None,
            requests: 0,
            listing: false,
            specs,
        }
    }

    /// Uses new command specifications, after they were read again.
    pub fn set_specs(&mut self, specs: Arc<Specs>) {
        self.specs = specs;
    }

    /// Brings the snapshot up to date if output arrived while the pane was
    /// hidden; call it before the pane is drawn again.
    pub fn show(&mut self) {
        if self.stale {
            self.refresh();
        }
    }

    /// The title the running program set, if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref().filter(|title| !title.is_empty())
    }

    /// Draws the pane again with a new font, after the configuration
    /// changed; the grid is measured again on its next layout. Colours are
    /// read when drawing, so this also applies new colours, and the new
    /// keyboard shortcuts.
    pub fn restyle(&mut self, metrics: Metrics, bindings: Arc<Bindings>) {
        self.bindings = bindings;
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

    /// The directory the shell reported it is in (ADR 0022), if it still
    /// exists, for new panes to start there.
    pub fn current_directory(&self) -> Option<std::path::PathBuf> {
        self.session
            .terminal()
            .current_directory()
            .map(std::path::PathBuf::from)
            .filter(|directory| directory.is_absolute() && directory.is_dir())
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
    /// output, also while the pane is not shown: the snapshot drawn from is
    /// then only taken once it is `shown` again, since it costs far more than
    /// a small update of the screen.
    pub fn process_output(&mut self, shown: bool) -> Task<Message> {
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
        if shown {
            self.refresh();
        } else {
            self.stale = true;
        }
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
        // Read from the terminal, not the snapshot, which a hidden pane does
        // not take.
        let terminal = self.session.terminal();
        let turned_on = self
            .gate
            .observe(terminal.awaiting_command(), terminal.command_prompt());
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
            Message::Key(KeyAction::Workspace(_) | KeyAction::Command(_)) => {}
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
                // A click or an input method moves away from what the list
                // completes.
                self.close_completion();
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
            Message::Command(command) => return self.run(command),
            Message::Listed(id, entries) => return self.listed(id, &entries, history),
            Message::CompletionClicked(index) => {
                if let Some(list) = &mut self.completion {
                    list.selected = index.min(list.candidates.len().saturating_sub(1));
                }
                return self.accept_completion(history);
            }
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
            bindings: Arc::clone(&self.bindings),
        }
    }

    /// Draws the pane. Only the `focused` pane takes keys.
    pub fn view<'a>(&'a self, palette: &'a Palette, focused: bool) -> Element<'a, Message> {
        let keys = self.key_context();
        let grid = canvas(TerminalView {
            screen: &self.screen,
            palette,
            metrics: self.metrics,
            keys: keys.clone(),
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
            completing: self.completion.is_some(),
        });
        match &self.completion {
            // Drawn over the bottom of the grid, so the grid keeps its size.
            Some(list) if focused => {
                let list = container(editor::completion_list(list, palette, self.metrics))
                    .height(Length::Fill)
                    .align_bottom(Length::Fill);
                column![stack![grid, list], panel].into()
            }
            _ => column![grid, panel].into(),
        }
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
        // What the list completes changed (a paste, for example).
        self.close_completion();
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
                ..keys.clone()
            };
            return match keys::key_press(&press.key, press.modifiers, press.text.as_deref(), &grid)
            {
                Some(action) => self.handle(Message::Key(action), history),
                None => Task::none(),
            };
        }
        // Keys reach the editor even when a click elsewhere took its focus;
        // the focus is given back so its cursor shows again.
        let refocus = !matches!(press.status, text_editor::Status::Focused { .. });
        press.status = text_editor::Status::Focused { is_hovered: false };

        let context = editor::context(&self.editor, keys, self.completion.is_some());
        let key = keys::editor_key(&press.key, press.modifiers, press.text.as_deref(), context);
        if !matches!(
            key,
            EditorKey::CompletionMove(_)
                | EditorKey::CompletionAccept
                | EditorKey::CompletionEnter
                | EditorKey::CompletionClose
        ) {
            // Any other key closes the list and acts as usual.
            self.close_completion();
        }
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
                if self.gate.submit(self.session.terminal().awaiting_command())
                    == gate::Submit::Confirm
                {
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
                if let Some(command) = history.previous(&current, self.shell).map(str::to_owned) {
                    self.set_editor_text(&command);
                }
            }
            EditorKey::HistoryNext => {
                if let Some(command) = history.next(self.shell).map(str::to_owned) {
                    self.set_editor_text(&command);
                }
            }
            EditorKey::Complete => return self.complete(history),
            EditorKey::CompletionMove(step) => {
                if let Some(list) = &mut self.completion {
                    list.select(step);
                }
            }
            EditorKey::CompletionAccept => return self.accept_completion(history),
            EditorKey::CompletionEnter => {
                if self
                    .completion
                    .as_ref()
                    .is_some_and(completion::List::enter_runs)
                {
                    self.close_completion();
                    return self.editor_key(EditorKey::Submit, history);
                }
                return self.accept_completion(history);
            }
            EditorKey::CompletionClose => self.close_completion(),
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

    /// Closes the completion list and forgets a completion waiting for its
    /// listing.
    fn close_completion(&mut self) {
        self.completion = None;
        self.pending = None;
    }

    /// Completes the word or command at the cursor (ADR 0023): paths from
    /// a listing read off the interface thread, and commands from the
    /// history.
    fn complete(&mut self, history: &mut History) -> Task<Message> {
        self.close_completion();
        let Some(shell) = self.shell else {
            return self.hand_off_tab(history);
        };
        let text = self.editor.text();
        let cursor = self.editor.cursor();
        let position = cursor.position;
        let line = self
            .editor
            .line(position.line)
            .map(|line| line.text.into_owned())
            .unwrap_or_default();
        let at_end = position.line + 1 >= self.editor.line_count() && position.column >= line.len();
        let commands = if at_end && cursor.selection.is_none() && !text.trim().is_empty() {
            completion::history_candidates(&text, history.newest_first(Some(shell)))
        } else {
            Vec::new()
        };
        let words = completion::words_before(&line, position.column, shell);
        let path = completion::PathRequest {
            line: position.line,
            word: completion::word_before(&line, position.column, shell),
            cursor: position.column,
            shell,
            ignore_case: cfg!(windows) || cfg!(target_os = "macos"),
            separator: if shell == Shell::PowerShell && cfg!(windows) {
                '\\'
            } else {
                '/'
            },
        };
        let free = cursor.selection.is_none();
        // An argument of a command with a specification (ADR 0024);
        // otherwise a path, except for the command itself.
        let spec = (free && words.len() >= 2)
            .then(|| self.specs.find(&words[0].value, cfg!(windows)))
            .flatten();
        let (known, lookup) = match spec {
            Some(spec) => {
                let suggestion = specs::suggest(spec, &words, &path);
                (suggestion.candidates, suggestion.lookup)
            }
            None if free && completes_paths(&line, &path) => (Vec::new(), Some(Arg::Path)),
            None => (Vec::new(), None),
        };
        let Some((job, want)) = lookup.and_then(|arg| self.lookup_for(&arg, &path)) else {
            return self.offer(known, commands, &path, false, history);
        };
        if self.listing {
            // The last listing has not returned (a slow or hung file
            // system): the shell's own completion is tried instead.
            return self.hand_off_tab(history);
        }
        self.listing = true;
        self.requests += 1;
        let id = self.requests;
        self.pending = Some(Pending {
            id,
            text,
            cursor: position,
            path,
            known,
            want,
            history: commands,
            asked: std::time::Instant::now(),
        });
        Task::perform(lookup_off_thread(job), move |entries| {
            Message::Listed(id, entries)
        })
    }

    /// What to read, off the interface thread, for an argument of kind
    /// `arg`; `None` when it cannot be known (no reported directory).
    fn lookup_for(&self, arg: &Arg, path: &completion::PathRequest) -> Option<(Lookup, Want)> {
        // bash, zsh and fish on Windows speak in `/c/Users` paths, which
        // Windows does not understand.
        let msys = cfg!(windows) && path.shell != Shell::PowerShell;
        let native = |text: &str| {
            msys.then(|| completion::msys_to_native(text))
                .flatten()
                .unwrap_or_else(|| text.to_owned())
        };
        let cwd = self
            .session
            .terminal()
            .current_directory()
            .map(|cwd| std::path::PathBuf::from(native(cwd)))
            .filter(|cwd| cwd.is_absolute());
        let git = |names| Some((Lookup::Git(cwd.clone()?, names), Want::Names));
        match arg {
            Arg::Path | Arg::File | Arg::Directory => {
                let (dir, _) = completion::split_path(&path.word.value, path.shell);
                let dir = native(dir);
                let directory = completion::directory_to_list(
                    &dir,
                    path.word.tilde,
                    cwd.as_deref(),
                    home_directory().as_deref(),
                    path.shell,
                )?;
                let only_directories = *arg == Arg::Directory;
                Some((
                    Lookup::Directory(directory),
                    Want::Paths { only_directories },
                ))
            }
            Arg::GitBranch => git(lookup::GitNames::Branches),
            Arg::GitTag => git(lookup::GitNames::Tags),
            Arg::GitRef => git(lookup::GitNames::Refs),
            Arg::GitRemote => git(lookup::GitNames::Remotes),
            Arg::SshHost => Some((Lookup::SshHosts(home_directory()?), Want::Names)),
            Arg::Text | Arg::Values(_) => None,
        }
    }

    /// A listing arrived: offers its completions if the editor still holds
    /// what it held when Tab was pressed.
    fn listed(
        &mut self,
        id: u64,
        entries: &[completion::Entry],
        history: &mut History,
    ) -> Task<Message> {
        // Only one listing runs at a time, so this is the one.
        self.listing = false;
        let current = self.pending.as_ref().is_some_and(|pending| {
            pending.id == id
                && self.editor_active()
                && self.editor.cursor().position == pending.cursor
                && self.editor.cursor().selection.is_none()
                && self.editor.text() == pending.text
        });
        let Some(pending) = self.pending.take().filter(|_| current) else {
            return Task::none();
        };
        let found = match pending.want {
            Want::Paths { only_directories } => {
                let mut paths = completion::path_candidates(&pending.path, entries);
                if only_directories {
                    paths.retain(|candidate| candidate.kind == completion::Kind::Directory);
                }
                paths
            }
            Want::Names => completion::word_candidates(
                &pending.path,
                entries.iter().map(|entry| entry.name.as_str()),
                completion::Kind::Value,
            ),
        };
        let mut candidates = pending.known;
        candidates.extend(found);
        // The user may have moved on, about to press Enter: a late result
        // opens the list instead of changing the line.
        let late = pending.asked.elapsed() > LATE_LISTING;
        self.offer(candidates, pending.history, &pending.path, late, history)
    }

    /// Does what [`completion::decide`] says: inserts the only completion,
    /// opens the list, or hands the text to the shell's own completion.
    fn offer(
        &mut self,
        words: Vec<completion::Candidate>,
        commands: Vec<completion::Candidate>,
        path: &completion::PathRequest,
        late: bool,
        history: &mut History,
    ) -> Task<Message> {
        match completion::decide(words, commands, path, late) {
            completion::Decision::HandOff => return self.hand_off_tab(history),
            completion::Decision::Nothing => {
                self.notice = Some("Nothing to complete");
            }
            completion::Decision::Insert(candidate) => self.apply(&candidate, history),
            completion::Decision::Open { insert, list } => {
                if let Some(common) = insert {
                    self.apply(&common, history);
                }
                self.completion = Some(list);
            }
        }
        Task::none()
    }

    /// Inserts the chosen completion and closes the list.
    fn accept_completion(&mut self, history: &mut History) -> Task<Message> {
        let chosen = self
            .completion
            .take()
            .and_then(|list| list.chosen().cloned());
        if let Some(candidate) = chosen {
            self.apply(&candidate, history);
        }
        Task::none()
    }

    /// Replaces the part of the editor's text a completion is for.
    fn apply(&mut self, candidate: &completion::Candidate, history: &mut History) {
        use text_editor::{Action, Cursor, Edit, Position};
        match candidate.span {
            completion::Span::All => {
                history.stop_walking();
                self.set_editor_text(&candidate.text);
            }
            completion::Span::Word { line, start, end } => {
                let replaced = end.saturating_sub(start);
                if !editor::fits(&self.editor, &candidate.text) && candidate.text.len() > replaced {
                    self.notice = Some("Too long to insert into the editor");
                    return;
                }
                self.editor.move_to(Cursor {
                    position: Position { line, column: end },
                    selection: (start != end).then_some(Position {
                        line,
                        column: start,
                    }),
                });
                self.edit(
                    Action::Edit(Edit::Paste(Arc::new(candidate.text.clone()))),
                    history,
                );
            }
        }
    }

    /// Hands the editor's text and a Tab to the shell's own completion.
    fn hand_off_tab(&mut self, history: &mut History) -> Task<Message> {
        let mode = self.session.terminal().mode();
        match input::encode_key(input::Key::Tab, input::Modifiers::default(), mode) {
            Some(bytes) => self.editor_key(EditorKey::HandOff(bytes), history),
            None => Task::none(),
        }
    }

    /// Runs a command from the command palette.
    fn run(&mut self, command: PaneCommand) -> Task<Message> {
        let last = self
            .session
            .terminal()
            .blocks()
            .iter()
            .rfind(|block| block.state() == woop_term::BlockState::Finished)
            .map(woop_term::Block::id);
        match (command, last) {
            (PaneCommand::ScrollToBottom, _) => self.scroll_to_bottom(),
            (PaneCommand::CopyLastCommand, Some(id)) => {
                // Read from what is displayed, as when copying from the bar.
                if let Some(command) = self.session.terminal().block_command(id) {
                    return clipboard::write(command);
                }
            }
            (PaneCommand::CopyLastOutput, Some(id)) => {
                // Selected first, so the user sees what was copied.
                if self.session.terminal_mut().select_block_output(id) {
                    return self.copy_grid_selection();
                }
            }
            (PaneCommand::ToggleLastOutput, Some(id)) => {
                if self.session.terminal_mut().toggle_collapsed(id).is_some() {
                    self.refresh();
                }
            }
            // No command has finished yet.
            (_, None) => self.notice = Some("No command has finished in this pane yet"),
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
        self.stale = false;
        self.screen = Terminal::screen(self.session.terminal());
        self.cache.clear();
    }
}

/// Whether the word before the cursor is completed as a path. A first word
/// without a separator names a command, which the shell completes better.
fn completes_paths(line: &str, path: &completion::PathRequest) -> bool {
    let value = &path.word.value;
    let before = line.get(..path.word.start).unwrap_or_default().trim_end();
    let command_position = before.is_empty() || before.ends_with([';', '|', '&', '(']);
    !command_position
        || path.word.tilde
        || value.starts_with('.')
        || value.contains(|ch| ch == '/' || (ch == '\\' && path.shell == Shell::PowerShell))
}

/// The user's home directory, for `~`.
fn home_directory() -> Option<std::path::PathBuf> {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(variable)
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_absolute())
}

/// Reads what `lookup` asks for on a thread of its own, so that a slow or
/// hung file system never blocks the window (ADR 0023, 0024).
async fn lookup_off_thread(lookup: Lookup) -> Arc<[completion::Entry]> {
    let names = |names: Vec<String>| {
        names
            .into_iter()
            .map(|name| completion::Entry {
                name,
                is_dir: false,
            })
            .collect()
    };
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("woop-completion".to_owned())
        .spawn(move || {
            let entries = match lookup {
                Lookup::Directory(directory) => completion::list_directory(&directory),
                Lookup::Git(start, which) => names(lookup::git_names(&start, which)),
                Lookup::SshHosts(home) => names(lookup::ssh_hosts(&home)),
            };
            let _ = sender.send(entries);
        });
    if started.is_err() {
        return Arc::from([]);
    }
    receiver.await.map_or_else(|_| Arc::from([]), Arc::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(line: &str, shell: Shell) -> completion::PathRequest {
        completion::PathRequest {
            line: 0,
            word: completion::word_before(line, line.len(), shell),
            cursor: line.len(),
            shell,
            ignore_case: false,
            separator: '/',
        }
    }

    #[test]
    fn commands_are_left_to_the_shell_and_arguments_are_paths() {
        let paths = |line: &str, shell| completes_paths(line, &request(line, shell));
        assert!(!paths("gi", Shell::Bash));
        assert!(!paths("", Shell::Bash));
        assert!(!paths("ls; gi", Shell::Bash));
        assert!(!paths("ls | gr", Shell::Zsh));
        assert!(paths("ls sr", Shell::Bash));
        assert!(paths("ls ", Shell::Bash));
        assert!(paths("./scr", Shell::Bash));
        assert!(paths("~/bin/x", Shell::Fish));
        assert!(paths("bin/x", Shell::Bash));
        assert!(paths(r"bin\x", Shell::PowerShell));
        assert!(!paths(r"bin\x", Shell::Bash));
    }
}
