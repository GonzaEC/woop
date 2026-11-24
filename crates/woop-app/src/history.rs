//! History of the commands submitted from Woop's editor (ADR 0016), and of
//! the shells' own history files (ADR 0023).
//!
//! Kept in memory only: commands can contain secrets, and nothing typed is
//! written to disk.

use std::collections::VecDeque;
use std::sync::Arc;

use woop_term::shell_integration::Shell;

/// Most commands kept.
const MAX_ENTRIES: usize = 1_000;

/// Longest command kept, in bytes. Longer ones (usually large pastes) run
/// but are not remembered, so the history stays small.
const MAX_ENTRY_BYTES: usize = 64 * 1024;

/// Submitted commands, oldest first, the commands read from each shell's
/// history file, and a position while walking them.
#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<String>,
    /// Commands read from a shell's own history file, oldest first; older
    /// than every entry typed in Woop.
    files: Vec<(Shell, Arc<[String]>)>,
    /// While walking the history, how many commands back the one shown is
    /// (1 is the newest), counting Woop's entries and then the shell's
    /// file; `None` when the editor holds the user's own draft. Counted
    /// from the newest so that a file read meanwhile does not move it.
    position: Option<usize>,
    /// What the editor held before walking started, restored at the end.
    draft: String,
}

impl History {
    /// Records a submitted command and ends any walk. Empty commands,
    /// commands starting with a space (a way to keep a secret out of the
    /// history), very long commands and repeats of the last one are not
    /// recorded.
    pub fn push(&mut self, command: &str) {
        self.position = None;
        self.draft.clear();
        if command.trim().is_empty()
            || command.starts_with(' ')
            || command.len() > MAX_ENTRY_BYTES
            || self.entries.back().is_some_and(|last| last == command)
        {
            return;
        }
        if self.entries.len() == MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(command.to_owned());
    }

    /// Sets the commands read from `shell`'s history file.
    pub fn set_file(&mut self, shell: Shell, commands: Arc<[String]>) {
        match self.files.iter_mut().find(|(known, _)| *known == shell) {
            Some((_, file)) => *file = commands,
            None => self.files.push((shell, commands)),
        }
    }

    fn file(&self, shell: Option<Shell>) -> &[String] {
        shell
            .and_then(|shell| self.files.iter().find(|(known, _)| *known == shell))
            .map_or(&[], |(_, file)| file)
    }

    /// The command `back` steps from the newest (1 is the newest).
    fn back(&self, back: usize, shell: Option<Shell>) -> Option<&str> {
        let mine = self.entries.len();
        if back == 0 {
            return None;
        }
        if back <= mine {
            return self.entries.get(mine - back).map(String::as_str);
        }
        let file = self.file(shell);
        file.len()
            .checked_sub(back - mine)
            .and_then(|index| file.get(index))
            .map(String::as_str)
    }

    /// Every command, newest first: Woop's, then `shell`'s file.
    pub fn newest_first(&self, shell: Option<Shell>) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .rev()
            .chain(self.file(shell).iter().rev())
            .map(String::as_str)
    }

    /// Returns the command before the one shown, given what the editor
    /// holds now, or `None` at the oldest command. `shell` is the pane's
    /// shell, whose history file follows Woop's own commands.
    pub fn previous(&mut self, current: &str, shell: Option<Shell>) -> Option<&str> {
        let back = self.position.map_or(1, |back| back + 1);
        self.back(back, shell)?;
        if self.position.is_none() {
            self.draft = current.to_owned();
        }
        self.position = Some(back);
        self.back(back, shell)
    }

    /// Returns the command after the one shown, or the draft the user was
    /// writing once past the newest command; `None` when not walking.
    pub fn next(&mut self, shell: Option<Shell>) -> Option<&str> {
        let back = self.position? - 1;
        if back > 0 {
            self.position = Some(back);
            self.back(back, shell)
        } else {
            self.position = None;
            Some(&self.draft)
        }
    }

    /// Ends a walk, keeping what the editor holds (after the user edits a
    /// command recalled from the history, for example).
    pub fn stop_walking(&mut self) {
        self.position = None;
        self.draft.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(commands: &[&str]) -> History {
        let mut history = History::default();
        for command in commands {
            history.push(command);
        }
        history
    }

    fn file(commands: &[&str]) -> Arc<[String]> {
        commands
            .iter()
            .map(|command| (*command).to_owned())
            .collect()
    }

    #[test]
    fn walks_back_and_forth_and_restores_the_draft() {
        let mut history = history(&["ls", "cd src", "git status"]);

        assert_eq!(history.previous("git di", None), Some("git status"));
        assert_eq!(history.previous("git status", None), Some("cd src"));
        assert_eq!(history.previous("cd src", None), Some("ls"));
        assert_eq!(history.previous("ls", None), None, "stops at the oldest");
        assert_eq!(history.next(None), Some("cd src"));
        assert_eq!(history.next(None), Some("git status"));
        assert_eq!(history.next(None), Some("git di"), "the draft comes back");
        assert_eq!(history.next(None), None, "not walking any more");
    }

    #[test]
    fn empty_history_recalls_nothing() {
        let mut history = History::default();
        assert_eq!(history.previous("draft", None), None);
        assert_eq!(history.next(None), None);
    }

    #[test]
    fn skips_empty_secret_long_and_repeated_commands() {
        let long = "x".repeat(MAX_ENTRY_BYTES + 1);
        let mut history = history(&["ls", "ls", "", "  ", " export TOKEN=1", &long, "pwd"]);

        assert_eq!(history.previous("", None), Some("pwd"));
        assert_eq!(history.previous("", None), Some("ls"));
        assert_eq!(history.previous("", None), None);
    }

    #[test]
    fn keeps_a_bounded_number_of_commands() {
        let mut history = History::default();
        for index in 0..MAX_ENTRIES + 10 {
            history.push(&format!("echo {index}"));
        }
        assert_eq!(history.entries.len(), MAX_ENTRIES);
        assert_eq!(history.entries.front().map(String::as_str), Some("echo 10"));
    }

    #[test]
    fn submitting_or_editing_ends_the_walk() {
        let mut history = history(&["one", "two"]);
        history.previous("draft", None);
        history.push("three");
        assert_eq!(history.next(None), None);
        assert_eq!(history.previous("", None), Some("three"));

        history.stop_walking();
        assert_eq!(history.next(None), None);
        assert_eq!(history.previous("edited", None), Some("three"));
    }

    #[test]
    fn the_shells_file_follows_woops_commands_for_its_shell_only() {
        let mut history = history(&["typed"]);
        history.set_file(Shell::Bash, file(&["oldest", "newest in file"]));

        assert_eq!(history.previous("draft", Some(Shell::Bash)), Some("typed"));
        assert_eq!(
            history.previous("", Some(Shell::Bash)),
            Some("newest in file")
        );
        assert_eq!(history.previous("", Some(Shell::Bash)), Some("oldest"));
        assert_eq!(history.previous("", Some(Shell::Bash)), None);
        assert_eq!(history.next(Some(Shell::Bash)), Some("newest in file"));
        assert_eq!(history.next(Some(Shell::Bash)), Some("typed"));
        assert_eq!(history.next(Some(Shell::Bash)), Some("draft"));

        assert_eq!(history.previous("", Some(Shell::Zsh)), Some("typed"));
        assert_eq!(history.previous("", Some(Shell::Zsh)), None);
        history.stop_walking();

        let all: Vec<_> = history.newest_first(Some(Shell::Bash)).collect();
        assert_eq!(all, ["typed", "newest in file", "oldest"]);
    }

    #[test]
    fn reading_a_file_during_a_walk_keeps_the_position() {
        let mut history = history(&["one", "two"]);
        assert_eq!(history.previous("", Some(Shell::Fish)), Some("two"));
        history.set_file(Shell::Fish, file(&["file"]));
        assert_eq!(history.previous("", Some(Shell::Fish)), Some("one"));
        assert_eq!(history.previous("", Some(Shell::Fish)), Some("file"));
    }
}
