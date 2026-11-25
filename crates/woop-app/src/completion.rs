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

/// Characters that make text display differently from what it is: control
/// characters, bidirectional and zero-width formatting, line separators.
pub fn is_deceptive(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{061c}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{2069}'
                | '\u{feff}'
        )
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
            let is_dir = file_type.is_dir()
                || (file_type.is_symlink()
                    && std::fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir()));
            Some(Entry { name, is_dir })
        })
        .collect()
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
    let starts = |name: &str| {
        if request.ignore_case {
            name.to_lowercase().starts_with(&lower_prefix)
        } else {
            name.starts_with(prefix)
        }
    };
    let mut matching: Vec<&Entry> = entries
        .iter()
        .filter(|entry| {
            (prefix.starts_with('.') || !entry.name.starts_with('.')) && starts(&entry.name)
        })
        .collect();
    matching.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.name.clone()));
    matching
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|entry| {
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
#[derive(Debug, Clone)]
pub struct List {
    /// The completions.
    pub candidates: Vec<Candidate>,
    /// The one chosen.
    pub selected: usize,
}

impl List {
    /// Moves the choice by `step`, wrapping around.
    pub fn select(&mut self, step: isize) {
        let len = self.candidates.len();
        if len > 0 {
            self.selected = (self.selected as isize + step).rem_euclid(len as isize) as usize;
        }
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
            '\u{202e}', '\u{2066}', '\u{200b}', '\u{2028}', '\u{feff}', '\u{061c}', '\n',
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
