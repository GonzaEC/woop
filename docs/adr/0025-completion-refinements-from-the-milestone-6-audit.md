# 0025. Refine completion after the milestone 6 audit

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

The end-of-milestone audit of completion ([ADR 0023](0023-history-and-path-completions.md), [ADR 0024](0024-command-specifications.md)) found problems that those decisions did not foresee:

- On Windows, a symbolic link to `\\host\share` in a listed directory or in a repository was followed. Opening it connects to that host and offers the user's credentials.
- bash on Windows reports its directory as `/c/Users`. This was dropped, so paths and git values never completed there, the main setup of Woop's own development.
- A list that opened late still took Enter, so a line the user was about to run changed instead.
- A matching history command kept a single path or subcommand from being inserted.
- Every positional argument of a command had the same kind, so `git push origin <Tab>` offered remotes again.
- The parsers of untrusted text had no property tests, which [ADR 0014](0014-property-based-robustness-tests.md) requires.

## Decision

- **No links to shares.** On Windows, completion never follows a symbolic link or junction to find out what it is:
  - a link in a listing is a directory if the link itself says so;
  - in a repository, `.git`, `packed-refs`, `config`, `commondir` and the ref directories are not read through links.

  git names are also held to git's own rules for ref names (no `\`, `..`, `//`, leading `/` and the like), so a crafted name cannot later be completed into a share's path.
- **Misleading characters, one definition.** `woop_config::spec::is_deceptive` covers:
  - control characters;
  - format characters: bidirectional controls, zero-width and invisible characters, tags;
  - variation selectors and characters that render blank.

  Listings, git and SSH names, specifications and history commands all use it. A history command containing one is left out rather than changed. Tabs stay in history commands.
- **bash, zsh and fish on Windows.** The scanner keeps a reported POSIX path (`/c/Users`) on every platform. Completion converts drive paths, `/x/...`, to `X:\...` for listing and for finding the repository. It does the same for absolute paths typed in those shells. Only drive paths are converted. New panes still do not start in a POSIX path ([ADR 0022](0022-working-directory-from-signed-marks.md)).
- **What Tab inserts.** `completion::decide`, a pure function, decides from the completions found:
  - A single completion of the word (a path, subcommand, option or value) is inserted, even if history commands also match.
  - The single history command is inserted only when nothing completes the word.
  - The shared beginning comes from the word completions alone. It is not inserted when it needs quotes, since typing on after the closing quote would make two words in PowerShell.
- **Late results.** A result that arrives after 300 ms never changes the line on its own:
  - one with nothing to offer shows a notice instead of handing the line to the shell;
  - an open list that came late lets Enter run the command, until the user moves the choice.

  Any edit to the text (a paste, for example) closes the list.
- **Arguments by position.** A specification can give `rest`: the kind of the second and later positional arguments, for example `args = "git-remote"` and `rest = "git-branch"`. Without it, `args` applies to all of them. After `--`, arguments complete as paths, whatever the command.
- **Property tests outside `woop-term`.** These properties run with the other robustness properties, also in the long runs:
  - `woop-app/src/robustness.rs` covers command lines, quoting round trips for every shell, history files of every format, and repository and SSH files;
  - `woop-config`'s spec tests cover specification files, parsed on a thread with a small stack.

  A test also runs every available shell on quoted names, and checks that the shell prints each name exactly.

## Alternatives considered

- **Resolving links and checking their targets.** Following a link only when its target is local would keep links to local directories marked as directories. But resolving a target is itself a file system call that can reach the share. Reading the link's own type is enough for completion.
- **Having bash report Windows paths (`pwd -W`).** That would fix completion, but new panes would then start in directories from a shell whose paths Woop does not otherwise trust. It would also tie the script to Git for Windows.
- **Keeping history first when it matches.** It suits users who repeat commands, but it makes Tab behave differently for the same word depending on the past. Up and Down remain the way to recall a command.

## Consequences

- Completion is safe to use in directories and repositories from archives on Windows. A link to a directory is still marked as a directory when the link says so.
- Git Bash users get path, git and specification values. A POSIX path that is not a drive (`/usr/bin` in MSYS2) is not completed.
- Specifications gain one optional field. The bundled git and kubectl specifications use it.
- The robustness properties now live in three crates. `AGENTS.md` points to all of them.
