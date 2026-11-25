//! Completions for the command editor: paths and history (ADR 0023).
//!
//! The functions here are pure, apart from [`list_directory`], which reads
//! the file system and runs off the interface thread.

use std::path::{Path, PathBuf};

use woop_term::shell_integration::Shell;

/// Most directory entries read for one completion.
const MAX_ENTRIES: usize = 10_000;

/// Most history commands offered.
const MAX_HISTORY: usize = 10;

/// Most completions kept in the list.
const MAX_CANDIDATES: usize = 1_000;

/// What a completion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A directory name, ending in a separator.
    Directory,
    /// A file name, or anything else that is not a directory.
    File,
    /// A command from the history.
    History,
    /// A subcommand, from a command specification (ADR 0024).
    Subcommand,
    /// An option, from a command specification.
    Option,
    /// A value for an argument: one listed in a specification, or a git
    /// ref or SSH host read from files.
    Value,
}

/// One completion: the text that replaces part of the editor's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// What it is.
    pub kind: Kind,
    /// What the list shows.
    pub label: String,
    /// What it does, from a specification; empty otherwise.
    pub description: String,
    /// The word inserted, before quoting.
    pub stem: String,
    /// The text inserted, already quoted for the shell.
    pub text: String,
    /// What it replaces.
    pub span: Span,
}

/// The part of the editor's text a completion replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// Bytes `start..end` of line `line`.
    Word {
        /// The line.
        line: usize,
        /// First byte replaced.
        start: usize,
        /// Byte after the last one replaced (the cursor).
        end: usize,
    },
    /// The whole text.
    All,
}

/// The word before the cursor, as the shell reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// Byte where the word starts in its line, quotes included.
    pub start: usize,
    /// The word with quotes and escapes removed.
    pub value: String,
    /// The word starts with an unquoted `~`, which the shell expands to the
    /// home directory.
    pub tilde: bool,
}

/// Reads the word that ends at byte `cursor` of `line`, with `shell`'s
/// quoting rules. Words are separated by spaces and by `;|&<>()` outside
/// quotes.
pub fn word_before(line: &str, cursor: usize, shell: Shell) -> Word {
    words_before(line, cursor, shell)
        .pop()
        .unwrap_or_else(|| Word {
            start: 0,
            value: String::new(),
            tilde: false,
        })
}

/// Reads the words of the command that ends at byte `cursor` of `line`:
/// those after the last `;|&<>()` outside quotes. The last one is the word
/// at the cursor, empty after a space.
pub fn words_before(line: &str, cursor: usize, shell: Shell) -> Vec<Word> {
    #[derive(Clone, Copy, PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let text = line.get(..cursor).unwrap_or(line);
    let escape = if shell == Shell::PowerShell {
        '`'
    } else {
        '\\'
    };
    let mut word = Word {
        start: 0,
        value: String::new(),
        tilde: false,
    };
    let mut words = Vec::new();
    let mut quote = Quote::None;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match quote {
            Quote::None => match ch {
                ' ' | '\t' | ';' | '|' | '&' | '<' | '>' | '(' | ')' => {
                    let next = Word {
                        start: index + ch.len_utf8(),
                        value: String::new(),
                        tilde: false,
                    };
                    let finished = std::mem::replace(&mut word, next);
                    if matches!(ch, ' ' | '\t') {
                        // Something, even empty quotes, since the last space.
                        if index > finished.start {
                            words.push(finished);
                        }
                    } else {
                        // A new command starts.
                        words.clear();
                    }
                }
                ch if is_single_quote(ch, shell) => quote = Quote::Single,
                ch if is_double_quote(ch, shell) => quote = Quote::Double,
                ch if ch == escape => {
                    if let Some((_, next)) = chars.next() {
                        word.value.push(next);
                    }
                }
                '~' if index == word.start => {
                    word.tilde = true;
                    word.value.push('~');
                }
                ch => word.value.push(ch),
            },
            Quote::Single => match ch {
                // PowerShell doubles a quote inside single quotes; any two
                // of its single quotes stand for the second.
                ch if shell == Shell::PowerShell
                    && is_single_quote(ch, shell)
                    && chars
                        .peek()
                        .is_some_and(|&(_, next)| is_single_quote(next, shell)) =>
                {
                    if let Some((_, next)) = chars.next() {
                        word.value.push(next);
                    }
                }
                ch if is_single_quote(ch, shell) => quote = Quote::None,
                // fish reads `\'` and `\\` inside single quotes.
                '\\' if shell == Shell::Fish
                    && chars
                        .peek()
                        .is_some_and(|&(_, next)| matches!(next, '\'' | '\\')) =>
                {
                    if let Some((_, next)) = chars.next() {
                        word.value.push(next);
                    }
                }
                ch => word.value.push(ch),
            },
            Quote::Double => match ch {
                ch if shell == Shell::PowerShell
                    && is_double_quote(ch, shell)
                    && chars
                        .peek()
                        .is_some_and(|&(_, next)| is_double_quote(next, shell)) =>
                {
                    if let Some((_, next)) = chars.next() {
                        word.value.push(next);
                    }
                }
                ch if is_double_quote(ch, shell) => quote = Quote::None,
                '`' if shell == Shell::PowerShell => {
                    if let Some((_, next)) = chars.next() {
                        word.value.push(next);
                    }
                }
                '\\' if shell != Shell::PowerShell
                    && chars
                        .peek()
                        .is_some_and(|&(_, next)| matches!(next, '"' | '\\' | '$' | '`')) =>
                {
                    if let Some((_, next)) = chars.next() {
                        word.value.push(next);
                    }
                }
                ch => word.value.push(ch),
            },
        }
    }
    words.push(word);
    words
}

/// Characters that start or end a single-quoted string in `shell`:
/// PowerShell also accepts the typographic single quotes.
fn is_single_quote(ch: char, shell: Shell) -> bool {
    ch == '\''
        || (shell == Shell::PowerShell
            && matches!(ch, '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}'))
}

/// Characters that start or end a double-quoted string in `shell`.
fn is_double_quote(ch: char, shell: Shell) -> bool {
    ch == '"' || (shell == Shell::PowerShell && matches!(ch, '\u{201c}' | '\u{201d}' | '\u{201e}'))
}

/// Characters that make text display differently from what it is
/// ([`woop_config::spec::is_deceptive`]).
pub fn is_deceptive(ch: char) -> bool {
    woop_config::spec::is_deceptive(ch)
}

/// Separators a path can use in `shell`.
fn is_separator(ch: char, shell: Shell) -> bool {
    ch == '/' || (ch == '\\' && shell == Shell::PowerShell)
}

/// A path word split into the directory part (up to and including the
/// last separator) and the beginning of a name.
pub fn split_path(value: &str, shell: Shell) -> (&str, &str) {
    match value.rfind(|ch| is_separator(ch, shell)) {
        Some(index) => value.split_at(index + 1),
        None => ("", value),
    }
}

/// The directory to list for the directory part `dir` of a word: relative
/// to `cwd`, or to `home` after a `~`. `None` when it cannot be known.
pub fn directory_to_list(
    dir: &str,
    tilde: bool,
    cwd: Option<&Path>,
    home: Option<&Path>,
    shell: Shell,
) -> Option<PathBuf> {
    if tilde {
        let rest = dir.strip_prefix('~')?;
        // `~user` is not expanded.
        if !rest.is_empty() && !rest.starts_with(|ch| is_separator(ch, shell)) {
            return None;
        }
        return Some(home?.join(rest.trim_start_matches(|ch| is_separator(ch, shell))));
    }
    let path = Path::new(dir);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    let cwd = cwd.filter(|cwd| cwd.is_absolute())?;
    Some(if dir.is_empty() {
        cwd.to_path_buf()
    } else {
        cwd.join(dir)
    })
}

/// A POSIX-style path of bash, zsh or fish on Windows (`/c/Users/me`, as
/// the shells of Git for Windows and MSYS2 report and accept it) as a
/// Windows path (`C:\Users\me`); `None` for any other path. Only drive
/// paths are converted, so a path to a network share cannot come out.
pub fn msys_to_native(path: &str) -> Option<String> {
    let rest = path.strip_prefix('/')?;
    let mut chars = rest.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let after = chars.as_str();
    if !(after.is_empty() || after.starts_with('/')) {
        return None;
    }
    Some(format!(
        "{}:\\{}",
        drive.to_ascii_uppercase(),
        after.trim_start_matches('/').replace('/', "\\")
    ))
}

/// One entry of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name.
    pub name: String,
    /// It is a directory, or a link to one.
    pub is_dir: bool,
}

/// Reads up to [`MAX_ENTRIES`] entries of `dir`. Names that are not valid
/// UTF-8 or that hold control characters are left out. Blocks on file
/// I/O: run it off the interface thread.
pub fn list_directory(dir: &Path) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .take(MAX_ENTRIES)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.chars().any(is_deceptive) {
                return None;
            }
            let file_type = entry.file_type().ok()?;
            let is_dir = file_type.is_dir() || (file_type.is_symlink() && link_is_dir(&entry));
            Some(Entry { name, is_dir })
        })
        .collect()
}

/// Whether a symbolic link points at a directory. On Windows this is read
/// from the link itself, never by following it: a link to `\\host\share`
/// would connect to that host and offer the user's credentials, just
/// because a directory holding it was listed.
#[cfg(windows)]
fn link_is_dir(entry: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::FileTypeExt;
    entry
        .file_type()
        .is_ok_and(|file_type| file_type.is_symlink_dir())
}

/// Whether a symbolic link points at a directory.
#[cfg(not(windows))]
fn link_is_dir(entry: &std::fs::DirEntry) -> bool {
    std::fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir())
}

/// Quotes `value` for `shell` if it holds anything other than letters,
/// digits and `-._/:+` (and `\` in PowerShell). A leading `~` of a home
/// path stays outside the quotes, so that the shell still expands it.
pub fn quote(value: &str, tilde: bool, shell: Shell) -> String {
    let (home, rest) = match tilde.then(|| value.strip_prefix('~')).flatten() {
        Some(rest) => ("~", rest),
        None => ("", value),
    };
    let safe = |ch: char| {
        ch.is_alphanumeric()
            || matches!(ch, '-' | '.' | '_' | '/' | ':' | '+')
            || (ch == '\\' && shell == Shell::PowerShell)
    };
    if rest.chars().all(safe) {
        return value.to_owned();
    }
    // Keep the separator after `~` outside the quotes as well.
    let (sep, rest) = match home.is_empty() {
        false if rest.starts_with(|ch| is_separator(ch, shell)) => rest.split_at(1),
        _ => ("", rest),
    };
    let quoted = match shell {
        // PowerShell ends a single-quoted string at any of its single
        // quotes, typographic ones included; each is doubled to stay
        // literal.
        Shell::PowerShell => {
            let mut quoted = String::with_capacity(rest.len() + 2);
            quoted.push('\'');
            for ch in rest.chars() {
                if is_single_quote(ch, shell) {
                    quoted.push(ch);
                }
                quoted.push(ch);
            }
            quoted.push('\'');
            quoted
        }
        Shell::Bash | Shell::Zsh => format!("'{}'", rest.replace('\'', r"'\''")),
        Shell::Fish => format!("'{}'", rest.replace('\\', r"\\").replace('\'', r"\'")),
    };
    format!("{home}{sep}{quoted}")
}

/// What a path completion needs to know about the word being completed.
#[derive(Debug, Clone)]
pub struct PathRequest {
    /// The line the word is on.
    pub line: usize,
    /// The word.
    pub word: Word,
    /// The cursor's byte in the line.
    pub cursor: usize,
    /// The shell, for separators and quoting.
    pub shell: Shell,
    /// Names are matched without regard to case.
    pub ignore_case: bool,
    /// The separator added after a directory when the word has none.
    pub separator: char,
}

impl PathRequest {
    /// The directory and name prefix of the word.
    fn parts(&self) -> (&str, &str) {
        split_path(&self.word.value, self.shell)
    }

    fn span(&self) -> Span {
        Span::Word {
            line: self.line,
            start: self.word.start,
            end: self.cursor,
        }
    }

    /// The separator for directories: the last one the word uses, or the
    /// default.
    fn separator(&self) -> char {
        self.word
            .value
            .chars()
            .rev()
            .find(|&ch| is_separator(ch, self.shell))
            .unwrap_or(self.separator)
    }
}

/// Paths in `entries` that complete the request's word, sorted by name.
pub fn path_candidates(request: &PathRequest, entries: &[Entry]) -> Vec<Candidate> {
    let (dir, prefix) = request.parts();
    let separator = request.separator();
    let lower_prefix = prefix.to_lowercase();
    // Compared without allocating: this runs over every entry, on the
    // interface thread.
    let starts = |name: &str| {
        if request.ignore_case {
            let mut lower = name.chars().flat_map(char::to_lowercase);
            lower_prefix.chars().all(|ch| lower.next() == Some(ch))
        } else {
            name.starts_with(prefix)
        }
    };
    let mut matching: Vec<(String, &Entry)> = entries
        .iter()
        .filter(|entry| {
            (prefix.starts_with('.') || !entry.name.starts_with('.')) && starts(&entry.name)
        })
        .map(|entry| (entry.name.to_lowercase(), entry))
        .collect();
    let order = |a: &(String, &Entry), b: &(String, &Entry)| {
        a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name))
    };
    // Only the kept ones are sorted.
    if matching.len() > MAX_CANDIDATES {
        matching.select_nth_unstable_by(MAX_CANDIDATES - 1, order);
        matching.truncate(MAX_CANDIDATES);
    }
    matching.sort_unstable_by(order);
    matching
        .into_iter()
        .map(|(_, entry)| {
            // A name starting with `-` would be read as an option.
            let here = if dir.is_empty() && !request.word.tilde && entry.name.starts_with('-') {
                format!(".{separator}")
            } else {
                String::new()
            };
            let mut value = format!("{here}{dir}{}", entry.name);
            let mut label = entry.name.clone();
            if entry.is_dir {
                value.push(separator);
                label.push(separator);
            }
            Candidate {
                kind: if entry.is_dir {
                    Kind::Directory
                } else {
                    Kind::File
                },
                label,
                description: String::new(),
                text: quote(&value, request.word.tilde, request.shell),
                stem: value,
                span: request.span(),
            }
        })
        .collect()
}

/// Words from `names` (sorted, without repeats) that complete the
/// request's word, as completions of `kind` with no description.
pub fn word_candidates<'a>(
    request: &PathRequest,
    names: impl IntoIterator<Item = &'a str>,
    kind: Kind,
) -> Vec<Candidate> {
    let prefix = request.word.value.as_str();
    let mut matching: Vec<&str> = names
        .into_iter()
        .filter(|name| name.starts_with(prefix))
        .collect();
    matching.sort_unstable();
    matching.dedup();
    matching
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|name| request.candidate(kind, name, name, ""))
        .collect()
}

impl PathRequest {
    /// A completion that replaces the word with `stem`, quoted.
    pub fn candidate(&self, kind: Kind, label: &str, stem: &str, description: &str) -> Candidate {
        Candidate {
            kind,
            label: label.to_owned(),
            description: description.to_owned(),
            stem: stem.to_owned(),
            text: quote(stem, self.word.tilde && stem.starts_with('~'), self.shell),
            span: self.span(),
        }
    }
}

/// When no candidate comes from the history and their words share a
/// beginning longer than what was typed, the completion that inserts that
/// beginning.
pub fn common_beginning(request: &PathRequest, candidates: &[Candidate]) -> Option<Candidate> {
    let first = candidates.first()?;
    if candidates
        .iter()
        .any(|candidate| candidate.kind == Kind::History)
    {
        return None;
    }
    let mut common: &str = &first.stem;
    for candidate in &candidates[1..] {
        let shared = common
            .char_indices()
            .zip(candidate.stem.chars())
            .find(|((_, a), b)| a != b)
            .map_or(common.len().min(candidate.stem.len()), |((index, _), _)| {
                index
            });
        common = &common[..shared];
    }
    // Typed in another case, the shared beginning only counts if it is
    // longer than what was typed.
    if common.chars().count() <= request.word.value.chars().count() {
        return None;
    }
    Some(request.candidate(Kind::File, common, common, ""))
}

/// Commands from `history` (newest first) that start with `text` and are
/// longer than it, without repeats.
pub fn history_candidates<'a>(
    text: &str,
    history: impl Iterator<Item = &'a str>,
) -> Vec<Candidate> {
    let mut seen = std::collections::HashSet::new();
    history
        .filter(|command| command.len() > text.len() && command.starts_with(text))
        .filter(|command| seen.insert(*command))
        .take(MAX_HISTORY)
        .map(|command| Candidate {
            kind: Kind::History,
            label: command.replace('\n', " \u{21b5} "),
            description: String::new(),
            stem: command.to_owned(),
            text: command.to_owned(),
            span: Span::All,
        })
        .collect()
}

/// The completion list while it is open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct List {
    /// The completions.
    pub candidates: Vec<Candidate>,
    /// The one chosen.
    pub selected: usize,
    /// It opened after the user may have moved on (a slow listing): Enter
    /// runs the command, as if the list were not there, until the user
    /// chooses an entry.
    pub late: bool,
    /// The user moved the choice.
    pub moved: bool,
}

/// What Tab does with the completions found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to offer: the shell's own completion is tried.
    HandOff,
    /// Nothing to offer, and too late to hand the line to the shell, which
    /// would change it under the user.
    Nothing,
    /// Insert this one.
    Insert(Candidate),
    /// Insert the shared beginning, if any, and open the list.
    Open {
        /// The shared beginning to insert first.
        insert: Option<Candidate>,
        /// The list, its spans already moved past that beginning.
        list: List,
    },
}

/// Decides what Tab does (ADR 0023, 0025). `words` complete the word at
/// the cursor (paths, subcommands, options, values) and `history` the
/// whole text. A single word completion is inserted even when the history
/// also matches, the history's single command only when no word does. The
/// shared beginning comes from the words alone, and is not inserted when it
/// needs quotes (typing on would leave the quote closed). A `late` result
/// changes nothing on its own: it only opens the list.
pub fn decide(
    mut words: Vec<Candidate>,
    mut history: Vec<Candidate>,
    request: &PathRequest,
    late: bool,
) -> Decision {
    if words.is_empty() && history.is_empty() {
        return if late {
            Decision::Nothing
        } else {
            Decision::HandOff
        };
    }
    if !late {
        if words.len() == 1 {
            return Decision::Insert(words.swap_remove(0));
        }
        if words.is_empty() && history.len() == 1 {
            return Decision::Insert(history.swap_remove(0));
        }
    }
    let insert = (!late && words.len() > 1)
        .then(|| common_beginning(request, &words))
        .flatten()
        .filter(|common| common.text == common.stem);
    let mut candidates = words;
    if let Some(common) = &insert
        && let Span::Word { start, .. } = common.span
    {
        // The word now ends after the shared beginning.
        let end = start + common.text.len();
        for candidate in &mut candidates {
            if let Span::Word { end: old, .. } = &mut candidate.span {
                *old = end;
            }
        }
    }
    candidates.extend(history);
    Decision::Open {
        insert,
        list: List {
            candidates,
            late,
            ..List::default()
        },
    }
}

impl List {
    /// Moves the choice by `step`, wrapping around.
    pub fn select(&mut self, step: isize) {
        let len = self.candidates.len();
        if len > 0 {
            self.selected = (self.selected as isize + step).rem_euclid(len as isize) as usize;
            self.moved = true;
        }
    }

    /// Whether Enter runs the command instead of inserting the chosen
    /// entry: the list came late and the user has not chosen anything.
    pub fn enter_runs(&self) -> bool {
        self.late && !self.moved
    }

    /// The chosen completion.
    pub fn chosen(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    /// The first index of the `rows` entries shown, keeping the chosen one
    /// in view.
    pub fn first_shown(&self, rows: usize) -> usize {
        let rows = rows.max(1);
        (self.selected + 1)
            .saturating_sub(rows)
            .min(self.candidates.len().saturating_sub(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(line: &str, shell: Shell) -> Word {
        word_before(line, line.len(), shell)
    }

    #[test]
    fn words_split_at_spaces_and_operators_outside_quotes() {
        assert_eq!(word("ls src/ma", Shell::Bash).value, "src/ma");
        assert_eq!(word("ls src/ma", Shell::Bash).start, 3);
        assert_eq!(word("cd a&&ls b|grep c", Shell::Bash).value, "c");
        assert_eq!(word("echo (dir", Shell::PowerShell).value, "dir");
        assert_eq!(word("ls ", Shell::Bash).value, "");
        assert_eq!(word("ls ", Shell::Bash).start, 3);
        assert_eq!(word("", Shell::Zsh).start, 0);
    }

    #[test]
    fn quotes_and_escapes_follow_the_shell() {
        assert_eq!(word("cat 'my dir'/f", Shell::Bash).value, "my dir/f");
        assert_eq!(word("cat 'my dir'/f", Shell::Bash).start, 4);
        assert_eq!(word(r"cat my\ dir/f", Shell::Zsh).value, "my dir/f");
        assert_eq!(
            word(r#"cat "a \"b\" \x""#, Shell::Bash).value,
            r#"a "b" \x"#
        );
        assert_eq!(word("cat 'it''s", Shell::PowerShell).value, "it's");
        assert_eq!(
            word(r"cat C:\Users\me", Shell::PowerShell).value,
            r"C:\Users\me"
        );
        assert_eq!(word("cat my` dir", Shell::PowerShell).value, "my dir");
        assert_eq!(word(r#"cat "a`"b"#, Shell::PowerShell).value, "a\"b");
        assert_eq!(word(r"cat 'it\'s \\ \n", Shell::Fish).value, r"it's \ \n");
        // An unfinished quote still gives its text.
        assert_eq!(word("cat 'my d", Shell::Bash).value, "my d");
    }

    #[test]
    fn only_a_leading_unquoted_tilde_means_home() {
        assert!(word("cd ~/sr", Shell::Bash).tilde);
        assert!(!word("cd '~'/sr", Shell::Bash).tilde);
        assert!(!word("cd a~b", Shell::Bash).tilde);
    }

    #[test]
    fn a_cursor_inside_the_line_ends_the_word() {
        let line = "ls src/ma rest";
        assert_eq!(word_before(line, 9, Shell::Bash).value, "src/ma");
        // Past the end, or not at a character boundary, the whole line.
        assert_eq!(word_before("ab", 10, Shell::Bash).value, "ab");
    }

    #[test]
    fn paths_split_at_the_shells_separators() {
        assert_eq!(split_path("src/ma", Shell::Bash), ("src/", "ma"));
        assert_eq!(split_path(r"src\ma", Shell::Bash), ("", r"src\ma"));
        assert_eq!(split_path(r"src\ma", Shell::PowerShell), (r"src\", "ma"));
        assert_eq!(split_path("ma", Shell::Fish), ("", "ma"));
    }

    #[test]
    fn directories_resolve_against_the_shell_or_home() {
        let (cwd, home) = if cfg!(windows) {
            (PathBuf::from(r"C:\work"), PathBuf::from(r"C:\Users\me"))
        } else {
            (PathBuf::from("/work"), PathBuf::from("/home/me"))
        };
        let list = |dir: &str, tilde: bool| {
            directory_to_list(dir, tilde, Some(&cwd), Some(&home), Shell::Bash)
        };
        assert_eq!(list("", false), Some(cwd.clone()));
        assert_eq!(list("src/", false), Some(cwd.join("src/")));
        assert_eq!(list("~/", true), Some(home.join("")));
        assert_eq!(list("~/a/", true), Some(home.join("a/")));
        assert_eq!(list("~", true), Some(home.join("")));
        assert_eq!(list("~other/", true), None, "~user is not expanded");
        // Without a reported directory, relative paths cannot be listed.
        assert_eq!(
            directory_to_list("src/", false, None, None, Shell::Bash),
            None
        );
        assert_eq!(
            directory_to_list("", false, Some(Path::new("relative")), None, Shell::Bash),
            None
        );
        let absolute = cwd.join("x").to_string_lossy().into_owned();
        assert_eq!(
            directory_to_list(&absolute, false, None, None, Shell::Bash),
            Some(PathBuf::from(&absolute))
        );
    }

    fn history(commands: &[&str]) -> Vec<Candidate> {
        history_candidates("", commands.iter().copied())
    }

    #[test]
    fn a_single_word_completion_is_inserted_even_with_history() {
        let request = request("git chec", Shell::Bash);
        let words = vec![request.candidate(Kind::Subcommand, "checkout", "checkout", "")];
        let decision = decide(
            words.clone(),
            history(&["git checkout main"]),
            &request,
            false,
        );
        assert_eq!(decision, Decision::Insert(words[0].clone()));
        // Only the history: its single command is inserted.
        let decision = decide(Vec::new(), history(&["git checkout main"]), &request, false);
        assert!(matches!(decision, Decision::Insert(c) if c.kind == Kind::History));
        // Several history commands open the list.
        let decision = decide(Vec::new(), history(&["a", "b"]), &request, false);
        assert!(matches!(decision, Decision::Open { insert: None, .. }));
    }

    #[test]
    fn the_shared_beginning_comes_from_words_and_moves_the_spans() {
        let listing = entries(&[("config.toml", false), ("config.json", false)]);
        let request = request("cat con", Shell::Bash);
        let words = path_candidates(&request, &listing);
        let Decision::Open { insert, list } = decide(words, history(&["cat x"]), &request, false)
        else {
            panic!("a list");
        };
        let insert = insert.expect("shared beginning");
        assert_eq!(insert.text, "config.");
        // Inserted over 4..7 ("con"): the word now ends at 4 + 7.
        assert_eq!(
            list.candidates[0].span,
            Span::Word {
                line: 0,
                start: 4,
                end: 11
            }
        );
        assert_eq!(list.candidates.last().map(|c| c.kind), Some(Kind::History));
        assert!(!list.enter_runs());
    }

    #[test]
    fn a_quoted_shared_beginning_is_not_inserted() {
        let listing = entries(&[("my file a", false), ("my file b", false)]);
        let request = request("cat m", Shell::Bash);
        let words = path_candidates(&request, &listing);
        assert!(matches!(
            decide(words, Vec::new(), &request, false),
            Decision::Open { insert: None, .. }
        ));
    }

    #[test]
    fn late_results_never_change_the_line_on_their_own() {
        let request = request("ls s", Shell::Bash);
        let one = vec![request.candidate(Kind::File, "src", "src", "")];
        let Decision::Open { insert, mut list } = decide(one, Vec::new(), &request, true) else {
            panic!("a list, even for one");
        };
        assert_eq!(insert, None);
        assert!(list.enter_runs(), "Enter runs the command");
        list.select(1);
        assert!(!list.enter_runs(), "once chosen, Enter inserts");
        assert_eq!(
            decide(Vec::new(), Vec::new(), &request, true),
            Decision::Nothing
        );
        assert_eq!(
            decide(Vec::new(), Vec::new(), &request, false),
            Decision::HandOff
        );
    }

    #[test]
    fn posix_drive_paths_become_windows_paths() {
        assert_eq!(
            msys_to_native("/c/Users/me").as_deref(),
            Some(r"C:\Users\me")
        );
        assert_eq!(msys_to_native("/d").as_deref(), Some(r"D:\"));
        assert_eq!(msys_to_native("/c/").as_deref(), Some(r"C:\"));
        for other in ["/usr/bin", "//host/share", "relative/c", "/1/x", "", "/"] {
            assert_eq!(msys_to_native(other), None, "{other}");
        }
    }

    #[test]
    fn quoting_round_trips_through_word_reading() {
        let names = [
            "plain",
            "my dir/",
            "it's",
            r"back\slash",
            "a\"b",
            "$HOME",
            "`tick`",
            "semi;colon",
            "@splat",
            "a,b",
            "ñandú",
            "-dash",
            "x\u{2019}; calc; \u{2019}.txt",
            "a\u{2018}b\u{201a}c\u{201b}d",
            "q\u{201c}u\u{201d}o\u{201e}te",
        ];
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
            for name in names {
                if shell == Shell::PowerShell && name.contains('\\') {
                    // A separator there, not part of a name.
                    continue;
                }
                let quoted = quote(name, false, shell);
                let line = format!("cat {quoted}");
                assert_eq!(word(&line, shell).value, name, "{shell:?} {quoted}");
            }
        }
    }

    /// Names that need quoting, for the real shells.
    const AWKWARD_NAMES: [&str; 14] = [
        "my dir",
        "it's",
        "a\"b",
        "$HOME",
        "`tick`",
        "semi;colon",
        "@splat",
        "a,b",
        "star*",
        "back\\slash",
        "ñandú",
        "x\u{2019}; echo INJECTED; \u{2019}.txt",
        "a\u{2018}b\u{201a}c\u{201b}d",
        "q\u{201c}u\u{201d}o\u{201e}te",
    ];

    /// Runs `script` in a real shell, if this machine has it, and returns
    /// its output lines.
    fn run_in(shell: Shell, script: &str) -> Option<Vec<String>> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let dir =
            std::env::temp_dir().join(format!("woop-quote-{:?}-{}", shell, std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let mut command = match shell {
            Shell::PowerShell => {
                // A file with a byte order mark, so that every version
                // reads it as UTF-8, and UTF-8 output.
                let file = dir.join("names.ps1");
                let text =
                    format!("\u{feff}[Console]::OutputEncoding = [Text.Encoding]::UTF8\n{script}");
                std::fs::write(&file, text).ok()?;
                let program = if cfg!(windows) {
                    "powershell.exe"
                } else {
                    "pwsh"
                };
                let mut command = Command::new(program);
                command
                    .args(["-NoProfile", "-NonInteractive", "-File"])
                    .arg(&file);
                command
            }
            // On Windows `bash` may name the Linux subsystem's; only Git
            // for Windows' bash is used there.
            Shell::Bash if cfg!(windows) => {
                let git_bash = std::path::Path::new(r"C:\Program Files\Git\bin\bash.exe");
                if !git_bash.exists() {
                    return None;
                }
                let mut command = Command::new(git_bash);
                command.args(["--noprofile", "--norc", "-s"]);
                command
            }
            Shell::Zsh | Shell::Fish if cfg!(windows) => return None,
            Shell::Bash => {
                let mut command = Command::new("bash");
                command.args(["--noprofile", "--norc", "-s"]);
                command
            }
            Shell::Zsh => {
                let mut command = Command::new("zsh");
                command.args(["-f", "-s"]);
                command
            }
            Shell::Fish => {
                let mut command = Command::new("fish");
                command.arg("--no-config");
                command
            }
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        if shell != Shell::PowerShell {
            child.stdin.take()?.write_all(script.as_bytes()).ok()?;
        } else {
            drop(child.stdin.take());
        }
        let output = child.wait_with_output().ok()?;
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8(output.stdout).ok()?;
        Some(
            text.lines()
                .map(|line| line.trim_end_matches('\r').to_owned())
                .collect(),
        )
    }

    /// Checked against the shells themselves, not against Woop's own
    /// reading of the quotes, which could share a mistake with them.
    #[test]
    fn real_shells_read_quoted_names_back() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
            let print = if shell == Shell::PowerShell {
                "Write-Output"
            } else {
                "printf '%s\\n'"
            };
            let script: String = AWKWARD_NAMES
                .iter()
                .map(|name| format!("{print} {}\n", quote(name, false, shell)))
                .collect();
            let Some(lines) = run_in(shell, &script) else {
                eprintln!("{shell:?} not available: quoting not checked in it");
                continue;
            };
            eprintln!("{shell:?}: quoting checked in the real shell");
            assert_eq!(lines, AWKWARD_NAMES, "{shell:?}\n{script}");
        }
    }

    #[test]
    fn powershell_doubles_every_single_quote_it_reads() {
        // PowerShell ends a single-quoted string at typographic quotes too;
        // checked against the text, not against Woop's own reading.
        assert_eq!(
            quote("x\u{2019}; calc; \u{2019}.txt", false, Shell::PowerShell),
            "'x\u{2019}\u{2019}; calc; \u{2019}\u{2019}.txt'"
        );
        assert_eq!(
            quote("'\u{2018}\u{201a}\u{201b}", false, Shell::PowerShell),
            "'''\u{2018}\u{2018}\u{201a}\u{201a}\u{201b}\u{201b}'"
        );
        // Double quotes are literal inside single quotes.
        assert_eq!(
            quote("a\u{201c}b", false, Shell::PowerShell),
            "'a\u{201c}b'"
        );
        // A word the user typed with a typographic quote is read as quoted.
        assert_eq!(
            word("cat \u{2018}my dir\u{2019}/f", Shell::PowerShell).value,
            "my dir/f"
        );
        assert_eq!(
            word("cat \u{201c}a b\u{201d}", Shell::PowerShell).value,
            "a b"
        );
        // Other shells treat them as ordinary characters, quoted as such.
        assert_eq!(quote("a\u{2019}b", false, Shell::Bash), "'a\u{2019}b'");
    }

    #[test]
    fn names_starting_with_a_dash_are_not_options() {
        let listing = entries(&[("-rf", false), ("-dir", true)]);
        let found = path_candidates(&request("rm -", Shell::Bash), &listing);
        let texts: Vec<_> = found.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["./-dir/", "./-rf"]);
        // With a directory part the name is not at the start of the word.
        let found = path_candidates(&request("rm a/-", Shell::Bash), &listing);
        assert_eq!(found[1].text, "a/-rf");
    }

    #[test]
    fn names_that_display_differently_are_deceptive() {
        for ch in [
            '\u{202e}',
            '\u{2066}',
            '\u{200b}',
            '\u{2028}',
            '\u{feff}',
            '\u{061c}',
            '\n',
            '\u{ad}',
            '\u{34f}',
            '\u{180e}',
            '\u{206a}',
            '\u{e0041}',
            '\u{fe0f}',
            '\u{115f}',
            '\u{3164}',
            '\u{ffa0}',
        ] {
            assert!(is_deceptive(ch), "{:?}", ch);
        }
        for ch in ['a', 'ñ', '\u{2019}', ' ', '漢'] {
            assert!(!is_deceptive(ch), "{:?}", ch);
        }
    }

    #[test]
    fn safe_names_are_not_quoted_and_home_stays_outside_quotes() {
        assert_eq!(quote("src/main.rs", false, Shell::Bash), "src/main.rs");
        assert_eq!(
            quote(r"C:\x\y.txt", false, Shell::PowerShell),
            r"C:\x\y.txt"
        );
        assert_eq!(quote("my dir/", false, Shell::Bash), "'my dir/'");
        assert_eq!(quote("~/my dir/", true, Shell::Bash), "~/'my dir/'");
        assert_eq!(quote("~/ok/", true, Shell::Zsh), "~/ok/");
        assert_eq!(quote("it's", false, Shell::Bash), r"'it'\''s'");
        assert_eq!(quote("it's", false, Shell::PowerShell), "'it''s'");
        assert_eq!(quote(r"it's \", false, Shell::Fish), r"'it\'s \\'");
        let line = format!("cd {}", quote("~/my dir/", true, Shell::Bash));
        let read = word(&line, Shell::Bash);
        assert!(read.tilde);
        assert_eq!(read.value, "~/my dir/");
    }

    fn entries(names: &[(&str, bool)]) -> Vec<Entry> {
        names
            .iter()
            .map(|&(name, is_dir)| Entry {
                name: name.to_owned(),
                is_dir,
            })
            .collect()
    }

    fn request(line: &str, shell: Shell) -> PathRequest {
        PathRequest {
            line: 0,
            word: word(line, shell),
            cursor: line.len(),
            shell,
            ignore_case: false,
            separator: '/',
        }
    }

    #[test]
    fn paths_complete_the_word_and_mark_directories() {
        let listing = entries(&[
            ("src", true),
            ("Cargo.toml", false),
            ("scripts", true),
            (".git", true),
            ("my file", false),
        ]);
        let request = request("ls s", Shell::Bash);
        let found = path_candidates(&request, &listing);
        let texts: Vec<_> = found.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["scripts/", "src/"]);
        assert_eq!(found[0].kind, Kind::Directory);
        assert_eq!(
            found[0].span,
            Span::Word {
                line: 0,
                start: 3,
                end: 4
            }
        );

        // Hidden names only when asked for; quoting where needed.
        let all = path_candidates(&request_for("ls ", Shell::Bash), &listing);
        assert!(all.iter().all(|c| !c.label.starts_with('.')));
        assert!(all.iter().any(|c| c.text == "'my file'"));
        let hidden = path_candidates(&request_for("ls .g", Shell::Bash), &listing);
        assert_eq!(hidden[0].text, ".git/");
    }

    fn request_for(line: &str, shell: Shell) -> PathRequest {
        request(line, shell)
    }

    #[test]
    fn directories_keep_the_separator_the_word_uses() {
        let listing = entries(&[("src", true)]);
        let mut request = request(r"ls a\s", Shell::PowerShell);
        request.separator = '\\';
        assert_eq!(path_candidates(&request, &listing)[0].text, r"a\src\");
        let mut request = self::request("ls a/s", Shell::PowerShell);
        request.separator = '\\';
        assert_eq!(path_candidates(&request, &listing)[0].text, "a/src/");
        let mut request = self::request("ls s", Shell::PowerShell);
        request.separator = '\\';
        assert_eq!(path_candidates(&request, &listing)[0].text, r"src\");
    }

    #[test]
    fn case_is_ignored_where_the_file_system_ignores_it() {
        let listing = entries(&[("Documents", true), ("downloads", true), ("Ñu", false)]);
        let mut request = request("cd do", Shell::PowerShell);
        assert_eq!(path_candidates(&request, &listing).len(), 1);
        request.ignore_case = true;
        assert_eq!(path_candidates(&request, &listing).len(), 2);
        let mut request = self::request("cat ñ", Shell::PowerShell);
        request.ignore_case = true;
        assert_eq!(path_candidates(&request, &listing).len(), 1);
    }

    #[test]
    fn the_shared_beginning_is_inserted_first() {
        let listing = entries(&[
            ("config.toml", false),
            ("config.json", false),
            ("cargo", true),
        ]);
        let request = request("cat con", Shell::Bash);
        let found = path_candidates(&request, &listing);
        let common = common_beginning(&request, &found).expect("shared beginning");
        assert_eq!(common.text, "config.");
        // Nothing longer than what was typed.
        let request = self::request("cat c", Shell::Bash);
        let found = path_candidates(&request, &listing);
        assert_eq!(common_beginning(&request, &found), None);
        // Quoted when the beginning needs it.
        let listing = entries(&[("my file a", false), ("my file b", false)]);
        let request = self::request("cat m", Shell::Bash);
        let found = path_candidates(&request, &listing);
        assert_eq!(
            common_beginning(&request, &found)
                .expect("shared beginning")
                .text,
            "'my file '"
        );
    }

    #[test]
    fn history_offers_longer_commands_newest_first_without_repeats() {
        let history = ["git status", "git stash", "git status", "git", "ls"];
        let found = history_candidates("git st", history.iter().copied());
        let texts: Vec<_> = found.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["git status", "git stash"]);
        assert_eq!(found[0].span, Span::All);
        let many: Vec<String> = (0..50).map(|index| format!("echo {index}")).collect();
        assert_eq!(
            history_candidates("echo", many.iter().map(String::as_str)).len(),
            MAX_HISTORY
        );
        let multi = history_candidates("for", ["for x\ndo\ndone"].into_iter());
        assert_eq!(multi[0].label, "for x \u{21b5} do \u{21b5} done");
    }

    #[test]
    fn the_list_wraps_and_keeps_the_choice_in_view() {
        let candidates = path_candidates(
            &request("ls ", Shell::Bash),
            &entries(&[("a", false), ("b", false), ("c", false)]),
        );
        let mut list = List {
            candidates,
            selected: 0,
            ..List::default()
        };
        list.select(-1);
        assert_eq!(list.selected, 2);
        list.select(1);
        assert_eq!(list.selected, 0);
        assert_eq!(list.first_shown(2), 0);
        list.selected = 2;
        assert_eq!(list.first_shown(2), 1);
        assert_eq!(list.first_shown(8), 0);
    }

    #[test]
    fn listing_leaves_out_names_that_cannot_be_typed() {
        let dir = std::env::temp_dir().join(format!("woop-listing-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).expect("test file system");
        std::fs::write(dir.join("file"), "").expect("test file system");
        // Displayed reversed: allowed by every file system, left out.
        std::fs::write(dir.join("txt.\u{202e}exe"), "").expect("test file system");
        // Control characters are not allowed in Windows names.
        if cfg!(unix) {
            std::fs::write(dir.join("bad\nname"), "").expect("test file system");
        }
        let mut listing = list_directory(&dir);
        listing.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(listing, entries(&[("file", false), ("sub", true)]));
        assert!(list_directory(&dir.join("missing")).is_empty());
        std::fs::remove_dir_all(&dir).expect("test file system");
    }
}
