//! Completions from command specifications (ADR 0024): subcommands,
//! options and the kind of value each argument takes.

use std::sync::Arc;

use woop_config::spec::{Arg, Command, Opt};

use crate::completion::{Candidate, Kind, PathRequest, Word};

/// The specifications in effect: the user's first, so that they replace
/// bundled ones with the same name.
#[derive(Debug, Default)]
pub struct Specs {
    user: Vec<Command>,
    /// Parsed once and shared, so reading the user's files again does not
    /// parse these again. Empty until the first specifications are loaded
    /// off the interface thread.
    bundled: Arc<[Command]>,
}

impl Specs {
    /// The user's specifications followed by the bundled ones. Parses the
    /// bundled ones the first time: call it off the interface thread.
    pub fn new(user: Vec<Command>) -> Arc<Self> {
        Arc::new(Self {
            user,
            bundled: woop_config::spec::bundled(),
        })
    }

    /// The specification for the command named by `word` (a path or a
    /// name with `.exe` and the like also match), if any. `ignore_case`
    /// for platforms whose command names ignore case.
    pub fn find(&self, word: &str, ignore_case: bool) -> Option<&Command> {
        let name = command_name(word);
        self.user.iter().chain(self.bundled.iter()).find(|command| {
            std::iter::once(&command.name)
                .chain(&command.aliases)
                .any(|known| {
                    if ignore_case {
                        known.eq_ignore_ascii_case(name)
                    } else {
                        known == name
                    }
                })
        })
    }
}

/// The command's name in `word`, without a directory or a Windows program
/// extension.
pub(crate) fn command_name(word: &str) -> &str {
    let file = word.rsplit(['/', '\\']).next().unwrap_or(word);
    for extension in [".exe", ".cmd", ".bat", ".com"] {
        if file.len() > extension.len()
            && file.is_char_boundary(file.len() - extension.len())
            && file[file.len() - extension.len()..].eq_ignore_ascii_case(extension)
        {
            return &file[..file.len() - extension.len()];
        }
    }
    file
}

/// What a specification offers for the word at the cursor.
#[derive(Debug, Default)]
pub struct Suggestion {
    /// Completions known without reading anything.
    pub candidates: Vec<Candidate>,
    /// A kind of value that must be read from files (paths, git refs, SSH
    /// hosts) to complete the word.
    pub lookup: Option<Arg>,
}

/// Completes the last of `words` (the command's words up to the cursor,
/// the first being its name) with `spec`. `request` describes that word.
pub fn suggest(spec: &Command, words: &[Word], request: &PathRequest) -> Suggestion {
    let Some((current, done)) = words.split_last() else {
        return Suggestion::default();
    };
    let mut node = spec;
    let mut globals: Vec<&Opt> = spec.options.iter().filter(|option| option.global).collect();
    // Positional arguments of the current command so far.
    let mut positionals = 0_usize;
    let mut expecting: Option<&Arg> = None;
    let mut options_ended = false;
    for word in done.iter().skip(1) {
        let value = word.value.as_str();
        if expecting.take().is_some() {
            continue;
        }
        if !options_ended && value == "--" {
            options_ended = true;
            continue;
        }
        if options_ended {
            positionals += 1;
            continue;
        }
        if value.len() > 1 && value.starts_with('-') {
            let (name, inline) = match value.split_once('=') {
                Some((name, _)) => (name, true),
                None => (value, false),
            };
            if let Some(option) = find_option(node, &globals, name)
                && !inline
            {
                expecting = option.args.as_ref();
            }
            continue;
        }
        if positionals == 0
            && let Some(sub) = node.subcommands.iter().find(|sub| sub.is_named(value))
        {
            node = sub;
            globals.extend(sub.options.iter().filter(|option| option.global));
            continue;
        }
        positionals += 1;
    }

    let prefix = current.value.as_str();
    if let Some(arg) = expecting {
        return for_arg(arg, request);
    }
    // After `--`, arguments are what most commands take there: paths
    // (`git checkout -- <file>`).
    if options_ended {
        return for_arg(&Arg::Path, request);
    }
    if prefix.starts_with('-') {
        return Suggestion {
            candidates: options(node, &globals, request),
            lookup: None,
        };
    }
    let arg = if positionals == 0 {
        node.args.as_ref()
    } else {
        node.rest.as_ref().or(node.args.as_ref())
    };
    let mut suggestion = arg.map_or_else(Suggestion::default, |arg| for_arg(arg, request));
    if positionals == 0 {
        let subcommands = node.subcommands.iter().filter_map(|sub| {
            let name = std::iter::once(&sub.name)
                .chain(&sub.aliases)
                .find(|name| name.starts_with(prefix))?;
            Some(request.candidate(Kind::Subcommand, name, name, &sub.description))
        });
        let mut candidates: Vec<Candidate> = subcommands.collect();
        candidates.append(&mut suggestion.candidates);
        suggestion.candidates = candidates;
    }
    suggestion
}

/// The option of `node`, or a global one of a command above it, named
/// `name`.
fn find_option<'a>(node: &'a Command, globals: &[&'a Opt], name: &str) -> Option<&'a Opt> {
    node.options
        .iter()
        .chain(globals.iter().copied())
        .find(|option| option.names.iter().any(|known| known == name))
}

/// Options whose names start with what was typed: long names after `--`,
/// otherwise the first name that matches. Each option is offered once.
fn options(node: &Command, globals: &[&Opt], request: &PathRequest) -> Vec<Candidate> {
    let prefix = request.word.value.as_str();
    let mut seen = std::collections::HashSet::new();
    node.options
        .iter()
        .chain(globals.iter().copied())
        .filter_map(|option| {
            let name = option
                .names
                .iter()
                .filter(|name| !prefix.starts_with("--") || name.starts_with("--"))
                .find(|name| name.starts_with(prefix))?;
            seen.insert(name.as_str()).then(|| {
                let label = option.names.join(", ");
                request.candidate(Kind::Option, &label, name, &option.description)
            })
        })
        .collect()
}

/// What completes an argument of kind `arg`.
fn for_arg(arg: &Arg, request: &PathRequest) -> Suggestion {
    match arg {
        Arg::Text => Suggestion::default(),
        Arg::Values(values) => Suggestion {
            candidates: values
                .iter()
                .filter(|value| value.starts_with(request.word.value.as_str()))
                .map(|value| request.candidate(Kind::Value, value, value, ""))
                .collect(),
            lookup: None,
        },
        other => Suggestion {
            candidates: Vec::new(),
            lookup: Some(other.clone()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::words_before;
    use woop_term::shell_integration::Shell;

    fn spec() -> Command {
        woop_config::spec::parse(
            r#"
            name = "tool"
            options = [
              { names = ["-C"], args = "directory" },
              { names = ["-v", "--verbose"], description = "Talk more", global = true },
            ]

            [[subcommands]]
            name = "pick"
            aliases = ["p"]
            description = "Pick one"
            args = ["apple", "apricot", "banana"]
            options = [
              { names = ["-m", "--message"], args = "text" },
              { names = ["--color"], args = ["auto", "never"] },
              { names = ["-b", "--branch"], args = "git-branch" },
            ]

            [[subcommands]]
            name = "push"
            args = "git-remote"
            rest = "git-branch"

            [[subcommands]]
            name = "copy"
            args = "path"
            "#,
        )
        .expect("valid")
    }

    fn complete(line: &str) -> Suggestion {
        let words = words_before(line, line.len(), Shell::Bash);
        let request = PathRequest {
            line: 0,
            word: words.last().cloned().expect("a word"),
            cursor: line.len(),
            shell: Shell::Bash,
            ignore_case: false,
            separator: '/',
        };
        suggest(&spec(), &words, &request)
    }

    fn texts(suggestion: &Suggestion) -> Vec<&str> {
        suggestion
            .candidates
            .iter()
            .map(|c| c.text.as_str())
            .collect()
    }

    #[test]
    fn subcommands_complete_after_the_command() {
        assert_eq!(texts(&complete("tool p")), ["pick", "push"]);
        let all = complete("tool ");
        assert_eq!(texts(&all), ["pick", "push", "copy"]);
        assert_eq!(all.candidates[0].kind, Kind::Subcommand);
        assert_eq!(all.candidates[0].description, "Pick one");
        assert!(complete("tool x").candidates.is_empty());
    }

    #[test]
    fn arguments_follow_the_subcommand_and_its_aliases() {
        assert_eq!(texts(&complete("tool pick ap")), ["apple", "apricot"]);
        assert_eq!(texts(&complete("tool p b")), ["banana"]);
        assert_eq!(complete("tool push ").lookup, Some(Arg::GitRemote));
        assert_eq!(complete("tool copy src/").lookup, Some(Arg::Path));
        // Global options before the subcommand do not get in the way.
        assert_eq!(texts(&complete("tool -v pick ba")), ["banana"]);
        // An option's value is skipped.
        assert_eq!(texts(&complete("tool -C /tmp pick ba")), ["banana"]);
        assert_eq!(texts(&complete("tool pick -m 'a b' ba")), ["banana"]);
    }

    #[test]
    fn options_complete_after_a_dash_including_global_ones() {
        let found = complete("tool pick -");
        assert_eq!(texts(&found), ["-m", "--color", "-b", "-v"]);
        assert_eq!(found.candidates[3].label, "-v, --verbose");
        assert_eq!(found.candidates[3].description, "Talk more");
        assert_eq!(
            texts(&complete("tool pick --")),
            ["--message", "--color", "--branch", "--verbose"]
        );
        assert_eq!(texts(&complete("tool pick --c")), ["--color"]);
        // Options of the top level do not apply after a subcommand, unless global.
        assert!(!texts(&complete("tool pick -")).contains(&"-C"));
    }

    #[test]
    fn option_values_complete_after_the_option() {
        assert_eq!(texts(&complete("tool pick --color ")), ["auto", "never"]);
        assert!(complete("tool pick -m ").candidates.is_empty());
        assert_eq!(complete("tool pick -b ").lookup, Some(Arg::GitBranch));
        assert_eq!(complete("tool -C ").lookup, Some(Arg::Directory));
        // A value given with `=` is complete.
        assert_eq!(texts(&complete("tool pick --color=auto ba")), ["banana"]);
    }

    #[test]
    fn after_a_positional_argument_or_double_dash() {
        // Subcommands are only offered before positional arguments.
        assert_eq!(texts(&complete("tool pick apple a")), ["apple", "apricot"]);
        assert!(texts(&complete("tool copy x p")).is_empty());
        // After `--`, words starting with `-` are arguments.
        assert_eq!(complete("tool copy -- -").lookup, Some(Arg::Path));
        // After `--`, paths whatever the arguments were before.
        assert_eq!(complete("tool pick -- ap").lookup, Some(Arg::Path));
        assert!(complete("tool pick -- ap").candidates.is_empty());
    }

    #[test]
    fn later_arguments_follow_rest() {
        assert_eq!(complete("tool push ").lookup, Some(Arg::GitRemote));
        assert_eq!(complete("tool push origin ").lookup, Some(Arg::GitBranch));
        assert_eq!(
            complete("tool push origin main ").lookup,
            Some(Arg::GitBranch)
        );
        // An option between them does not count as an argument.
        assert_eq!(
            complete("tool push -v origin ").lookup,
            Some(Arg::GitBranch)
        );
        // Without `rest`, the first kind repeats.
        assert_eq!(complete("tool copy a b ").lookup, Some(Arg::Path));
    }

    #[test]
    fn commands_are_found_by_name_path_or_extension() {
        let specs = Specs::new(vec![spec()]);
        assert!(specs.find("tool", false).is_some());
        assert!(specs.find("/usr/bin/tool", false).is_some());
        assert!(specs.find(r"C:\bin\tool.exe", false).is_some());
        assert!(specs.find("Tool.EXE", true).is_some());
        assert!(specs.find("Tool", false).is_none());
        assert!(specs.find("git", false).is_some(), "bundled");
        assert!(specs.find("pip3", false).is_some(), "by alias");
        assert!(specs.find("nothing", false).is_none());
    }

    #[test]
    fn user_specifications_replace_bundled_ones() {
        let mine = woop_config::spec::parse("name = \"git\"\n[[subcommands]]\nname = \"mine\"")
            .expect("valid");
        let specs = Specs::new(vec![mine]);
        let git = specs.find("git", false).expect("git");
        assert_eq!(git.subcommands[0].name, "mine");
    }

    #[test]
    fn a_command_after_an_operator_is_read_on_its_own() {
        let words = words_before("cd x && tool pi", 15, Shell::Bash);
        let names: Vec<_> = words.iter().map(|word| word.value.as_str()).collect();
        assert_eq!(names, ["tool", "pi"]);
    }
}
