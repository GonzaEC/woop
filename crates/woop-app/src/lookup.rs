//! Values for command arguments read from files, never from programs (ADR
//! 0024): git branches, tags and remotes from the repository's own files,
//! and hosts from the user's SSH configuration.
//!
//! Everything here blocks on file I/O and runs off the interface thread.
//! Every read is bounded, and names that could display as something other
//! than what they are are left out.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::completion::is_deceptive;

/// Most names returned by one lookup.
const MAX_NAMES: usize = 10_000;

/// Largest file read (packed refs, git and SSH configuration).
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Most directories walked up looking for a repository.
const MAX_PARENTS: usize = 64;

/// Deepest nesting of ref directories walked (`feature/a/b`).
const MAX_REF_DEPTH: usize = 8;

/// Which git names to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitNames {
    /// Local branches, and remote ones as `remote/branch`.
    Branches,
    /// Tags.
    Tags,
    /// Branches and tags.
    Refs,
    /// Remotes.
    Remotes,
}

/// Names of the git repository that contains `start`.
pub fn git_names(start: &Path, which: GitNames) -> Vec<String> {
    let Some(git) = git_dir(start) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut budget = MAX_VISITS;
    let branches = matches!(which, GitNames::Branches | GitNames::Refs);
    let tags = matches!(which, GitNames::Tags | GitNames::Refs);
    if branches {
        list_refs(
            &git.join("refs").join("heads"),
            "",
            0,
            &mut names,
            &mut budget,
        );
        list_refs(
            &git.join("refs").join("remotes"),
            "",
            0,
            &mut names,
            &mut budget,
        );
        // `origin/HEAD` points at another branch.
        names.retain(|name| !name.ends_with("/HEAD"));
    }
    if tags {
        list_refs(
            &git.join("refs").join("tags"),
            "",
            0,
            &mut names,
            &mut budget,
        );
    }
    if (branches || tags)
        && let Some(packed) = read_bounded(&git.join("packed-refs"))
    {
        names.extend(packed_refs(&packed, branches, tags));
    }
    if which == GitNames::Remotes
        && let Some(config) = read_bounded(&git.join("config"))
    {
        names.extend(remotes(&config));
    }
    // Names git itself could not have, such as `//host/share`, are not
    // offered: a later Tab could otherwise list that path.
    names.retain(|name| is_ref_name(name));
    clean(names)
}

/// The directory holding the repository's refs, for the repository that
/// contains `start`: `.git`, or the directory a `.git` file names (linked
/// working trees and submodules), or its common directory.
fn git_dir(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors().take(MAX_PARENTS) {
        let dot_git = dir.join(".git");
        if !may_follow(&dot_git) {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&dot_git) else {
            continue;
        };
        let git = if metadata.is_dir() {
            dot_git
        } else {
            let text = read_bounded(&dot_git)?;
            let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
            local_join(dir, target)?
        };
        // A linked working tree keeps its refs in the common directory.
        return match read_bounded(&git.join("commondir")) {
            Some(common) => local_join(&git, common.trim()),
            None => Some(git),
        };
    }
    None
}

/// `base` joined with `target` from a repository's own files, unless the
/// target names a network share or a device. On Windows, opening
/// `\\host\share` connects to that host and offers the user's credentials;
/// a crafted `.git` file must not make a Tab do that.
fn local_join(base: &Path, target: &str) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    if target.starts_with("\\\\") || target.starts_with("//") {
        return None;
    }
    let path = Path::new(target);
    let local = path.components().all(|component| match component {
        Component::Prefix(prefix) => matches!(prefix.kind(), Prefix::Disk(_)),
        _ => true,
    });
    local.then(|| base.join(path))
}

/// Whether a repository file may be opened through `path`. On Windows a
/// symbolic link or junction is never followed: one pointing at
/// `\\host\share` would connect to that host and offer the user's
/// credentials when a Tab reads the repository.
#[cfg(windows)]
fn may_follow(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_symlink())
}

/// Whether a repository file may be opened through `path`.
#[cfg(not(windows))]
fn may_follow(_path: &Path) -> bool {
    true
}

/// Whether git could have a ref with this name (the main rules of
/// `git check-ref-format`): no `\`, `..`, `//`, `@{`, spaces or `:?*[~^`,
/// no leading or trailing `/`, no trailing `.` or `.lock`.
pub(crate) fn is_ref_name(name: &str) -> bool {
    !name.is_empty()
        && name != "@"
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with('.')
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name.contains(['\\', ':', '?', '*', '[', '~', '^'])
        && !name.split('/').any(|part| part.starts_with('.'))
}

/// Most entries visited under the ref directories of one lookup, files and
/// directories alike, so that a tree of empty directories cannot keep the
/// thread busy.
const MAX_VISITS: usize = 50_000;

/// Adds the names of the ref files under `dir`, as `prefix/name`, visiting
/// at most `budget` more entries.
fn list_refs(dir: &Path, prefix: &str, depth: usize, names: &mut Vec<String>, budget: &mut usize) {
    if depth > MAX_REF_DEPTH || names.len() >= MAX_NAMES || *budget == 0 || !may_follow(dir) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if names.len() >= MAX_NAMES || *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                list_refs(&entry.path(), &full, depth + 1, names, budget);
            }
            Ok(kind) if kind.is_file() => names.push(full),
            _ => {}
        }
    }
}

/// Branch and tag names from a `packed-refs` file.
pub(crate) fn packed_refs(text: &str, branches: bool, tags: bool) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .filter_map(|line| line.split_once(' ').map(|(_, name)| name.trim()))
        .filter_map(|name| {
            if branches {
                if let Some(branch) = name
                    .strip_prefix("refs/heads/")
                    .or_else(|| name.strip_prefix("refs/remotes/"))
                {
                    return Some(branch.to_owned());
                }
            }
            if tags {
                return name.strip_prefix("refs/tags/").map(str::to_owned);
            }
            None
        })
        .collect()
}

/// Remote names from a git configuration file: `[remote "name"]`.
pub(crate) fn remotes(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('[')?.trim_start();
            let rest = rest.strip_prefix("remote")?.trim_start();
            let rest = rest.strip_prefix('"')?;
            let (name, end) = rest.split_once('"')?;
            end.trim_start().starts_with(']').then(|| name.to_owned())
        })
        .collect()
}

/// Host names from the user's SSH configuration (`~/.ssh/config`), without
/// patterns.
pub fn ssh_hosts(home: &Path) -> Vec<String> {
    read_bounded(&home.join(".ssh").join("config")).map_or_else(Vec::new, |text| hosts(&text))
}

/// Host names from the text of an SSH configuration, without patterns.
pub(crate) fn hosts(text: &str) -> Vec<String> {
    let hosts = text.lines().flat_map(|line| {
        let line = line.trim();
        let (keyword, rest) = line
            .split_once(|ch: char| ch.is_whitespace() || ch == '=')
            .unwrap_or((line, ""));
        let values = if keyword.eq_ignore_ascii_case("host") {
            rest.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '=')
        } else {
            ""
        };
        values
            .split_whitespace()
            .filter(|host| !host.contains(['*', '?', '!']))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    clean(hosts.collect())
}

/// Sorts, removes repeats and leaves out names that cannot be typed as one
/// word or that would display as something else.
pub(crate) fn clean(mut names: Vec<String>) -> Vec<String> {
    names.retain(|name| {
        // A name starting with `-` would be read as an option; real refs,
        // remotes and hosts never do.
        !name.is_empty()
            && !name.starts_with('-')
            && name.len() <= 255
            && !name
                .chars()
                .any(|ch| ch.is_whitespace() || is_deceptive(ch))
    });
    names.sort_unstable();
    names.dedup();
    names.truncate(MAX_NAMES);
    names
}

/// A regular file's text, at most [`MAX_FILE_BYTES`] of it; `None` for
/// anything else (missing, a directory, a pipe or a device).
fn read_bounded(path: &Path) -> Option<String> {
    if !may_follow(path) || !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("woop-lookup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create");
        std::fs::write(path, text).expect("write");
    }

    #[test]
    fn reads_branches_tags_and_remotes_without_running_git() {
        let repo = temp("repo");
        let git = repo.join(".git");
        write(&git.join("refs/heads/main"), "0\n");
        write(&git.join("refs/heads/feature/login"), "0\n");
        write(
            &git.join("refs/remotes/origin/HEAD"),
            "ref: refs/remotes/origin/main\n",
        );
        write(&git.join("refs/remotes/origin/main"), "0\n");
        write(&git.join("refs/tags/v1.0"), "0\n");
        write(
            &git.join("packed-refs"),
            "# pack-refs with: peeled\nabc refs/heads/old\nabc refs/tags/v0.9\n^def\n",
        );
        write(
            &git.join("config"),
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = x\n[remote \"fork\"]\n[branch \"main\"]\n",
        );
        let inside = repo.join("src").join("deep");
        std::fs::create_dir_all(&inside).expect("create");

        assert_eq!(
            git_names(&inside, GitNames::Branches),
            ["feature/login", "main", "old", "origin/main"]
        );
        assert_eq!(git_names(&inside, GitNames::Tags), ["v0.9", "v1.0"]);
        assert_eq!(git_names(&inside, GitNames::Refs).len(), 6);
        assert_eq!(git_names(&inside, GitNames::Remotes), ["fork", "origin"]);
        std::fs::remove_dir_all(&repo).expect("remove");
    }

    #[test]
    fn linked_working_trees_read_the_common_directory() {
        let root = temp("worktree");
        let main = root.join("main").join(".git");
        write(&main.join("refs/heads/main"), "0\n");
        write(&main.join("worktrees/wt/commondir"), "../..\n");
        let tree = root.join("wt");
        write(
            &tree.join(".git"),
            &format!("gitdir: {}\n", main.join("worktrees/wt").display()),
        );
        assert_eq!(git_names(&tree, GitNames::Branches), ["main"]);
        std::fs::remove_dir_all(&root).expect("remove");
    }

    #[test]
    fn a_broken_git_file_gives_no_names() {
        // The nearest `.git` decides; a broken one is not looked past, so
        // this holds even if the temporary directory sits in a repository.
        let dir = temp("broken");
        write(&dir.join(".git"), "not a gitdir line\n");
        assert!(git_names(&dir, GitNames::Branches).is_empty());
        write(&dir.join(".git"), "gitdir: missing/dir\n");
        assert!(git_names(&dir, GitNames::Branches).is_empty());
        std::fs::remove_dir_all(&dir).expect("remove");
    }

    #[test]
    fn ssh_hosts_without_patterns() {
        let home = temp("ssh");
        write(
            &home.join(".ssh/config"),
            "Host web db.internal\n  User me\nHost *.example.com\nhost=build\nHost !bad ok\n# Host commented\nMatch host x\n",
        );
        assert_eq!(ssh_hosts(&home), ["build", "db.internal", "ok", "web"]);
        assert!(ssh_hosts(&home.join("nobody")).is_empty());
        std::fs::remove_dir_all(&home).expect("remove");
    }

    #[test]
    fn names_that_could_mislead_are_left_out() {
        let names = clean(vec![
            "ok".into(),
            "two words".into(),
            "rtl\u{202e}txt".into(),
            "bell\u{7}".into(),
            String::new(),
            "x".repeat(300),
            "ok".into(),
        ]);
        assert_eq!(names, ["ok"]);
    }

    #[test]
    fn git_files_cannot_point_at_network_shares_or_devices() {
        let base = Path::new("base");
        for target in [
            r"\\host\share\x",
            "//host/share/x",
            r"\\?\UNC\host\s",
            r"\\.\pipe\p",
        ] {
            assert_eq!(local_join(base, target), None, "{target}");
        }
        assert_eq!(local_join(base, "../x"), Some(base.join("../x")));
        if cfg!(windows) {
            assert!(local_join(base, r"C:\repo\.git").is_some());
        }
        assert!(clean(vec!["--orphan".into(), "-x".into()]).is_empty());
    }

    #[test]
    fn only_names_git_could_have_are_offered() {
        for good in ["main", "feature/login", "v1.0", "origin/main", "a-b_c"] {
            assert!(is_ref_name(good), "{good}");
        }
        for bad in [
            "//host/share/x",
            r"a\b",
            "a..b",
            "/lead",
            "trail/",
            "x:y",
            "x.lock",
            "end.",
            "a/.hidden",
            "@",
            "a@{1}",
            "~x",
        ] {
            assert!(!is_ref_name(bad), "{bad}");
        }
        assert_eq!(
            packed_refs("0 refs/heads///host/share\n0 refs/heads/ok\n", true, false),
            ["//host/share", "ok"],
            "read as written, then refused by git_names"
        );
    }

    #[cfg(windows)]
    #[test]
    fn links_in_a_repository_are_not_followed_on_windows() {
        let repo = temp("links");
        let git = repo.join(".git");
        write(&git.join("refs/heads/main"), "0\n");
        let target = repo.join("elsewhere");
        write(&target.join("packed-refs"), "0 refs/heads/planted\n");
        // Creating links needs a privilege the machine may not grant.
        if std::os::windows::fs::symlink_file(target.join("packed-refs"), git.join("packed-refs"))
            .is_ok()
        {
            assert!(!may_follow(&git.join("packed-refs")));
            assert_eq!(git_names(&repo, GitNames::Branches), ["main"]);
        }
        std::fs::remove_dir_all(&repo).expect("remove");
    }

    #[test]
    fn remote_sections_are_read_strictly() {
        let found = remotes(
            "[remote \"a\"]\n[ remote \"b\" ]\n[remotes \"c\"]\n[remote d]\n[remote \"e\" x]\n",
        );
        assert_eq!(found, ["a", "b"]);
    }
}
