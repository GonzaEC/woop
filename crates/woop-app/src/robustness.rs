//! Property tests for the code that reads untrusted text in the interface
//! (ADR 0014): command lines and quoting, shell history files, and the
//! files of a repository or SSH configuration read for completion. Like
//! `woop-term/tests/robustness.rs`, `PROPTEST_CASES` raises the number of
//! cases for the long runs at the end of a milestone.

use proptest::prelude::*;
use woop_term::shell_integration::Shell;

use crate::completion::{is_deceptive, quote, words_before};
use crate::{lookup, shell_history, specs};

fn shell() -> impl Strategy<Value = Shell> {
    prop_oneof![
        Just(Shell::Bash),
        Just(Shell::Zsh),
        Just(Shell::Fish),
        Just(Shell::PowerShell),
    ]
}

/// Text with the characters shells treat specially, plus anything else.
fn shell_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[ a-z'\"`\\\\$~/;|&()<>\u{2018}\u{2019}\u{201c}\u{201d}-]{0,40}",
        any::<String>(),
    ]
}

/// A name a file could have, as shown: no characters that display
/// differently from what they are (those are left out before quoting).
fn name() -> impl Strategy<Value = String> {
    prop_oneof![
        "[ a-zA-Z0-9'\"`\\\\$~/;|&()<>!*?#=@%^.,:+\u{2018}\u{2019}\u{201a}\u{201b}\u{201c}\u{201d}\u{201e}ñ漢-]{0,30}",
        any::<String>(),
    ]
    .prop_filter("shown as it is", |name| !name.chars().any(is_deceptive))
}

proptest! {
    #[test]
    fn reading_any_command_line_never_panics(line in shell_text(), cursor in 0_usize..64, shell in shell()) {
        let words = words_before(&line, cursor, shell);
        prop_assert!(!words.is_empty());
        let end = line.get(..cursor).map_or(line.len(), str::len);
        for word in &words {
            prop_assert!(word.start <= end);
        }
    }

    /// What is inserted is read back by the shell as the name shown, as
    /// one word: a quoting mistake would name a different file.
    #[test]
    fn quoted_names_read_back_as_themselves(name in name(), shell in shell()) {
        let line = format!("cat {}", quote(&name, false, shell));
        let words = words_before(&line, line.len(), shell);
        prop_assert_eq!(words.len(), 2, "{}", line);
        prop_assert_eq!(&words[1].value, &name, "{}", line);
        prop_assert!(!words[1].tilde);
    }

    #[test]
    fn home_paths_keep_their_tilde(rest in name(), shell in shell()) {
        let separator = if shell == Shell::PowerShell { '\\' } else { '/' };
        let value = format!("~{separator}{rest}");
        let line = format!("cd {}", quote(&value, true, shell));
        let words = words_before(&line, line.len(), shell);
        prop_assert_eq!(words.len(), 2, "{}", line);
        prop_assert_eq!(&words[1].value, &value, "{}", line);
        prop_assert!(words[1].tilde, "{}", line);
    }

    /// History files can be written by anyone on a shared machine that
    /// gets the permissions wrong, or copied from elsewhere.
    #[test]
    fn any_history_file_gives_clean_commands(
        bytes in prop_oneof![any::<Vec<u8>>(), "[: 0-9;a-z\\\\\n`\u{83}-]{0,200}".prop_map(String::into_bytes)],
        shell in shell(),
    ) {
        for command in shell_history::parse(shell, &bytes) {
            prop_assert!(!command.trim().is_empty());
            prop_assert!(crate::editor::holds(&command));
            prop_assert!(!command.chars().any(|ch| !matches!(ch, '\n' | '\t') && is_deceptive(ch)), "{:?}", command);
        }
    }

    /// A repository's files come from whoever made the archive or the
    /// repository.
    #[test]
    fn repository_and_ssh_files_give_only_plain_names(text in any::<String>(), branches: bool, tags: bool) {
        let mut names = lookup::packed_refs(&text, branches, tags);
        names.extend(lookup::remotes(&text));
        names.retain(|name| lookup::is_ref_name(name));
        names.extend(lookup::hosts(&text));
        for name in lookup::clean(names) {
            prop_assert!(!name.is_empty() && !name.starts_with('-'));
            prop_assert!(!name.chars().any(|ch| ch.is_whitespace() || is_deceptive(ch)), "{:?}", name);
        }
    }

    #[test]
    fn walking_any_command_line_with_a_specification_never_panics(line in shell_text(), shell in shell()) {
        let specs = specs::Specs::new(Vec::new());
        let words = words_before(&line, line.len(), shell);
        let _ = specs::command_name(&words[0].value);
        if let Some(word) = words.last()
            && let Some(spec) = specs.find("git", false)
        {
            let request = crate::completion::PathRequest {
                line: 0,
                word: word.clone(),
                cursor: line.len(),
                shell,
                ignore_case: false,
                separator: '/',
            };
            let mut command = vec![words[0].clone()];
            command[0].value = "git".to_owned();
            command.extend(words.iter().skip(1).cloned());
            let suggestion = specs::suggest(spec, &command, &request);
            for candidate in suggestion.candidates {
                prop_assert!(!candidate.text.is_empty());
            }
        }
    }
}
