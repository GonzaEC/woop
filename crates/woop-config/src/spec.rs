//! Command specifications for completion (ADR 0024): the subcommands,
//! options and arguments of a command.
//!
//! Specifications come with Woop for common commands, and users can add
//! their own as TOML files in the `completions` directory next to
//! `config.toml`, which replace a bundled one with the same name. A
//! specification only describes a command; it never runs anything.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::ConfigError;

/// Longest name of a command, option or value, in characters.
const MAX_NAME_CHARS: usize = 64;

/// Longest description, in characters.
const MAX_DESCRIPTION_CHARS: usize = 200;

/// Most commands, options and values in one file.
const MAX_ITEMS: usize = 20_000;

/// Deepest nesting of tables and arrays in a file: deeper files are
/// refused before parsing, so that parsing cannot exhaust the stack.
const MAX_NESTING: usize = 24;

/// Most specification files read from the user's directory.
const MAX_FILES: usize = 256;

/// A command, or one of its subcommands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Command {
    /// The name typed.
    pub name: String,
    /// Other names for it.
    pub aliases: Vec<String>,
    /// What it does, shown next to it.
    pub description: String,
    /// Its options.
    pub options: Vec<Opt>,
    /// Its subcommands.
    pub subcommands: Vec<Command>,
    /// What its positional arguments are.
    pub args: Option<Arg>,
}

/// An option of a command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Opt {
    /// Its spellings, such as `-m` and `--message`.
    pub names: Vec<String>,
    /// What it does.
    pub description: String,
    /// The value it takes, if any, as the next word.
    pub args: Option<Arg>,
    /// It also applies after a subcommand.
    pub global: bool,
}

/// What an argument is, which decides what completes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    /// Any path.
    Path,
    /// A file (directories are offered to walk into).
    File,
    /// A directory.
    Directory,
    /// Free text: nothing to complete.
    Text,
    /// One of these values.
    Values(Vec<String>),
    /// A branch of the git repository the shell is in.
    GitBranch,
    /// A tag of the git repository.
    GitTag,
    /// A branch or a tag.
    GitRef,
    /// A remote of the git repository.
    GitRemote,
    /// A host from the user's SSH configuration.
    SshHost,
}

impl Command {
    /// Whether `word` names this command.
    pub fn is_named(&self, word: &str) -> bool {
        self.name == word || self.aliases.iter().any(|alias| alias == word)
    }
}

/// The specifications that come with Woop.
pub fn bundled() -> Vec<Command> {
    BUNDLED
        .iter()
        .filter_map(|(_, text)| parse(text).ok())
        .collect()
}

/// Bundled specification files, by name.
const BUNDLED: &[(&str, &str)] = &[
    ("cargo", include_str!("../specs/cargo.toml")),
    ("docker", include_str!("../specs/docker.toml")),
    ("gh", include_str!("../specs/gh.toml")),
    ("git", include_str!("../specs/git.toml")),
    ("kubectl", include_str!("../specs/kubectl.toml")),
    ("npm", include_str!("../specs/npm.toml")),
    ("pip", include_str!("../specs/pip.toml")),
    ("rustup", include_str!("../specs/rustup.toml")),
    ("scp", include_str!("../specs/scp.toml")),
    ("ssh", include_str!("../specs/ssh.toml")),
];

/// The directory of the user's own specifications, next to the
/// configuration file.
pub fn user_dir() -> Option<PathBuf> {
    Some(crate::path()?.parent()?.join("completions"))
}

/// Reads the user's specifications from `dir`: every `.toml` file, in
/// name order, at most [`MAX_FILES`]. Files that cannot be used are
/// reported and skipped. A missing directory gives none. Blocks on file
/// I/O: run it off the interface thread.
pub fn load_dir(dir: &Path) -> (Vec<Command>, Vec<ConfigError>) {
    let mut errors = Vec::new();
    // A directory others can write could hold planted files; checked as the
    // parent of a file in it.
    if let Err(error) = crate::check_private_dir(&dir.join("spec.toml")) {
        return (Vec::new(), vec![error]);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (Vec::new(), errors);
    };
    let mut files: Vec<PathBuf> = entries
        .take(MAX_FILES * 4)
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    files.sort();
    files.truncate(MAX_FILES);
    let mut specs = Vec::new();
    for file in files {
        let shown = file.display().to_string();
        match crate::read_private(&file) {
            Ok(Some(text)) => match parse(&text) {
                Ok(spec) => specs.push(spec),
                Err(ConfigError::Invalid(message)) => {
                    errors.push(ConfigError::Invalid(format!("{shown}: {message}")));
                }
                Err(error) => errors.push(error),
            },
            Ok(None) => {}
            Err(error) => errors.push(error),
        }
    }
    (specs, errors)
}

/// Parses and validates a specification.
pub fn parse(text: &str) -> Result<Command, ConfigError> {
    if nesting(text) > MAX_NESTING {
        return Err(crate::invalid(format!(
            "nested more than {MAX_NESTING} levels deep"
        )));
    }
    let raw: RawCommand =
        toml::from_str(text).map_err(|error| crate::invalid(error.to_string()))?;
    let mut items = 0;
    convert(raw, &mut items, 0)
}

/// How deeply tables and arrays nest in TOML `text`, counting the dotted
/// parts of table headers and the brackets and braces outside strings and
/// comments. An over-estimate is fine: it only refuses odd files.
fn nesting(text: &str) -> usize {
    let mut deepest = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            let header = trimmed.split(']').next().unwrap_or_default();
            deepest = deepest.max(header.matches('.').count() + 1);
        }
    }
    let mut depth: usize = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '#' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                while let Some(next) = chars.next() {
                    match next {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '\'' => {
                for next in chars.by_ref() {
                    if next == '\'' {
                        break;
                    }
                }
            }
            '[' | '{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    options: Vec<RawOpt>,
    #[serde(default)]
    subcommands: Vec<RawCommand>,
    args: Option<RawArg>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOpt {
    names: Vec<String>,
    #[serde(default)]
    description: String,
    args: Option<RawArg>,
    #[serde(default)]
    global: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawArg {
    Kind(String),
    Values(Vec<String>),
}

fn count(items: &mut usize, more: usize) -> Result<(), ConfigError> {
    *items += more;
    if *items > MAX_ITEMS {
        return Err(crate::invalid(format!(
            "more than {MAX_ITEMS} commands, options and values"
        )));
    }
    Ok(())
}

fn convert(raw: RawCommand, items: &mut usize, depth: usize) -> Result<Command, ConfigError> {
    if depth > MAX_NESTING {
        return Err(crate::invalid(format!(
            "subcommands nested more than {MAX_NESTING} levels deep"
        )));
    }
    count(items, 1 + raw.aliases.len())?;
    let name = name(raw.name)?;
    let aliases = raw
        .aliases
        .into_iter()
        .map(self::name)
        .collect::<Result<_, _>>()?;
    let description = description(raw.description)?;
    let options = raw
        .options
        .into_iter()
        .map(|option| {
            count(items, option.names.len())?;
            if option.names.is_empty() {
                return Err(crate::invalid(format!("an option of {name} has no names")));
            }
            Ok(Opt {
                names: option
                    .names
                    .into_iter()
                    .map(self::name)
                    .collect::<Result<_, _>>()?,
                description: self::description(option.description)?,
                args: option.args.map(|arg| arg_kind(arg, items)).transpose()?,
                global: option.global,
            })
        })
        .collect::<Result<_, _>>()?;
    let subcommands = raw
        .subcommands
        .into_iter()
        .map(|sub| convert(sub, items, depth + 1))
        .collect::<Result<_, _>>()?;
    Ok(Command {
        args: raw.args.map(|arg| arg_kind(arg, items)).transpose()?,
        name,
        aliases,
        description,
        options,
        subcommands,
    })
}

fn arg_kind(raw: RawArg, items: &mut usize) -> Result<Arg, ConfigError> {
    Ok(match raw {
        RawArg::Values(values) => {
            count(items, values.len())?;
            Arg::Values(values.into_iter().map(name).collect::<Result<_, _>>()?)
        }
        RawArg::Kind(kind) => match kind.as_str() {
            "path" => Arg::Path,
            "file" => Arg::File,
            "directory" => Arg::Directory,
            "text" => Arg::Text,
            "git-branch" => Arg::GitBranch,
            "git-tag" => Arg::GitTag,
            "git-ref" => Arg::GitRef,
            "git-remote" => Arg::GitRemote,
            "ssh-host" => Arg::SshHost,
            other => {
                return Err(crate::invalid(format!(
                    "unknown argument kind {other:?}; use path, file, directory, text, \
                     git-branch, git-tag, git-ref, git-remote, ssh-host or a list of values"
                )));
            }
        },
    })
}

/// A name or value: one word that displays as it is.
fn name(value: String) -> Result<String, ConfigError> {
    if value.is_empty()
        || value.chars().count() > MAX_NAME_CHARS
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || is_deceptive(ch))
    {
        return Err(crate::invalid(format!(
            "{:?} is not a valid name: one word of at most {MAX_NAME_CHARS} printable characters",
            value.chars().take(MAX_NAME_CHARS).collect::<String>()
        )));
    }
    Ok(value)
}

fn description(value: String) -> Result<String, ConfigError> {
    if value.chars().count() > MAX_DESCRIPTION_CHARS || value.chars().any(is_deceptive) {
        return Err(crate::invalid(format!(
            "a description is longer than {MAX_DESCRIPTION_CHARS} characters or holds control characters"
        )));
    }
    Ok(value)
}

/// Characters that make text display differently from what it is.
fn is_deceptive(ch: char) -> bool {
    ch.is_control()
        || crate::is_direction_override(ch)
        || matches!(
            ch,
            '\u{061c}' | '\u{200b}'..='\u{200d}' | '\u{2028}' | '\u{2029}' | '\u{2060}'..='\u{2065}' | '\u{feff}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_specification_is_valid() {
        for (file, text) in BUNDLED {
            let spec = parse(text).unwrap_or_else(|error| panic!("{file}: {error}"));
            assert_eq!(spec.name, *file);
            assert!(
                !spec.subcommands.is_empty() || !spec.options.is_empty(),
                "{file}"
            );
        }
        assert_eq!(bundled().len(), BUNDLED.len());
    }

    #[test]
    fn bundled_names_are_unique_per_command() {
        fn check(command: &Command, path: &str) {
            let mut seen = std::collections::HashSet::new();
            for sub in &command.subcommands {
                for name in std::iter::once(&sub.name).chain(&sub.aliases) {
                    assert!(seen.insert(name.clone()), "{path} {name} twice");
                }
                check(sub, &format!("{path} {}", sub.name));
            }
            let mut options = std::collections::HashSet::new();
            for option in &command.options {
                for name in &option.names {
                    assert!(name.starts_with('-'), "{path} {name}");
                    assert!(options.insert(name.clone()), "{path} {name} twice");
                }
            }
        }
        for spec in bundled() {
            check(&spec, &spec.name);
        }
    }

    #[test]
    fn parses_subcommands_options_and_argument_kinds() {
        let spec = parse(
            r#"
            name = "tool"
            aliases = ["t"]
            description = "A tool"
            options = [{ names = ["-C"], args = "directory", global = true }]

            [[subcommands]]
            name = "pick"
            args = ["one", "two"]
            options = [{ names = ["-m", "--message"], description = "Say it", args = "text" }]

            [[subcommands.subcommands]]
            name = "deep"
            args = "git-branch"
            "#,
        )
        .expect("valid");
        assert!(spec.is_named("t"));
        assert_eq!(spec.options[0].args, Some(Arg::Directory));
        assert!(spec.options[0].global);
        let pick = &spec.subcommands[0];
        assert_eq!(
            pick.args,
            Some(Arg::Values(vec!["one".into(), "two".into()]))
        );
        assert_eq!(pick.options[0].names, ["-m", "--message"]);
        assert_eq!(pick.subcommands[0].args, Some(Arg::GitBranch));
    }

    #[test]
    fn refuses_what_could_mislead_or_exhaust() {
        let refused = [
            ("name = \"two words\"", "valid name"),
            ("name = \"a\u{202e}b\"", "valid name"),
            ("name = \"\"", "valid name"),
            ("name = \"x\"\nargs = \"programs\"", "unknown argument kind"),
            ("name = \"x\"\ncolor = 1", "unknown field"),
            ("name = \"x\"\noptions = [{ names = [] }]", "no names"),
            // Escaped, so TOML accepts it and Woop's own check refuses it.
            ("name = \"x\"\ndescription = \"bell\\u0007\"", "control"),
            ("name = \"x\"\ndescription = \"rtl\\u202e\"", "control"),
        ];
        for (text, expected) in refused {
            let error = parse(text).expect_err(text).to_string();
            assert!(error.contains(expected), "{text:?}: {error}");
        }
        let long = format!("name = \"{}\"", "x".repeat(MAX_NAME_CHARS + 1));
        assert!(parse(&long).is_err());
    }

    #[test]
    fn deep_nesting_is_refused_before_parsing() {
        let header = format!("[[{}]]\nname = \"x\"\n", vec!["subcommands"; 200].join("."));
        let text = format!("name = \"x\"\n{header}");
        assert!(
            parse(&text)
                .expect_err("deep")
                .to_string()
                .contains("nested")
        );
        let inline = format!(
            "name = \"x\"\nargs = {}{}",
            "[".repeat(5_000),
            "]".repeat(5_000)
        );
        assert!(
            parse(&inline)
                .expect_err("deep")
                .to_string()
                .contains("nested")
        );
        // Brackets in strings and comments do not count.
        let text = format!(
            "# {}\nname = \"x\"\ndescription = \"{}\"\n",
            "[".repeat(100),
            "[".repeat(100)
        );
        assert!(parse(&text).is_ok());
        assert_eq!(nesting("a = \"[[[[\"\nb = '{{{{'"), 0);
    }

    #[test]
    fn too_many_items_are_refused() {
        let values: Vec<String> = (0..=MAX_ITEMS)
            .map(|index| format!("\"v{index}\""))
            .collect();
        let text = format!("name = \"x\"\nargs = [{}]", values.join(","));
        assert!(
            parse(&text)
                .expect_err("many")
                .to_string()
                .contains("more than")
        );
    }

    #[test]
    fn user_files_are_read_in_order_and_errors_reported() {
        let dir = std::env::temp_dir().join(format!("woop-specs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("b.toml"), "name = \"b\"").expect("write");
        std::fs::write(dir.join("a.toml"), "name = \"a\"").expect("write");
        std::fs::write(dir.join("bad.toml"), "name = 1").expect("write");
        std::fs::write(dir.join("notes.txt"), "ignored").expect("write");
        let (specs, errors) = load_dir(&dir);
        let names: Vec<_> = specs.iter().map(|spec| spec.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("bad.toml"), "{}", errors[0]);
        let (none, no_errors) = load_dir(&dir.join("missing"));
        assert!(none.is_empty() && no_errors.is_empty());
        std::fs::remove_dir_all(&dir).expect("remove");
    }
}
