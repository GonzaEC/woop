//! History of the commands submitted from Woop's editor (ADR 0016).
//!
//! Kept in memory only: commands can contain secrets, and nothing typed is
//! written to disk.

use std::collections::VecDeque;

/// Most commands kept.
const MAX_ENTRIES: usize = 1_000;

/// Longest command kept, in bytes. Longer ones (usually large pastes) run
/// but are not remembered, so the history stays small.
const MAX_ENTRY_BYTES: usize = 64 * 1024;

/// Submitted commands, oldest first, and a position while walking them.
#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<String>,
    /// Index of the entry shown in the editor while walking the history;
    /// `None` when the editor holds the user's own draft.
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

    /// Returns the command before the one shown, given what the editor
    /// holds now, or `None` at the oldest command.
    pub fn previous(&mut self, current: &str) -> Option<&str> {
        let index = match self.position {
            None => {
                self.draft = current.to_owned();
                self.entries.len().checked_sub(1)?
            }
            Some(index) => index.checked_sub(1)?,
        };
        self.position = Some(index);
        self.entries.get(index).map(String::as_str)
    }

    /// Returns the command after the one shown, or the draft the user was
    /// writing once past the newest command; `None` when not walking.
    pub fn next(&mut self) -> Option<&str> {
        let index = self.position? + 1;
        if index < self.entries.len() {
            self.position = Some(index);
            self.entries.get(index).map(String::as_str)
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

    #[test]
    fn walks_back_and_forth_and_restores_the_draft() {
        let mut history = history(&["ls", "cd src", "git status"]);

        assert_eq!(history.previous("git di"), Some("git status"));
        assert_eq!(history.previous("git status"), Some("cd src"));
        assert_eq!(history.previous("cd src"), Some("ls"));
        assert_eq!(history.previous("ls"), None, "stops at the oldest");
        assert_eq!(history.next(), Some("cd src"));
        assert_eq!(history.next(), Some("git status"));
        assert_eq!(history.next(), Some("git di"), "the draft comes back");
        assert_eq!(history.next(), None, "not walking any more");
    }

    #[test]
    fn empty_history_recalls_nothing() {
        let mut history = History::default();
        assert_eq!(history.previous("draft"), None);
        assert_eq!(history.next(), None);
    }

    #[test]
    fn skips_empty_secret_long_and_repeated_commands() {
        let long = "x".repeat(MAX_ENTRY_BYTES + 1);
        let mut history = history(&["ls", "ls", "", "  ", " export TOKEN=1", &long, "pwd"]);

        assert_eq!(history.previous(""), Some("pwd"));
        assert_eq!(history.previous(""), Some("ls"));
        assert_eq!(history.previous(""), None);
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
        history.previous("draft");
        history.push("three");
        assert_eq!(history.next(), None);
        assert_eq!(history.previous(""), Some("three"));

        history.stop_walking();
        assert_eq!(history.next(), None);
        assert_eq!(history.previous("edited"), Some("three"));
    }
}
