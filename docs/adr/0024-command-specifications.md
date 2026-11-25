# 0024. Complete subcommands and options from command specifications

- **Status:** Accepted
- **Date:** 2026-11-23

## Context

Tab completes paths and history ([ADR 0023](0023-history-and-path-completions.md)), but it knows nothing about a command's own structure. `git ch<Tab>` should offer `checkout` and `cherry-pick`, `cargo build --r<Tab>` should offer `--release`, and `git switch <Tab>` should offer branches.

Some of this knowledge is fixed: the subcommands and options of a command. Some depends on where the shell is, such as a repository's branches or the hosts the user connects to. Running a program to find these out is risky. Inside a hostile repository, git can be made to run code from its configuration, and opening a shell there is enough to trigger the completion. The specifications themselves come from Woop and from the user; collections maintained by other projects cannot be copied without their licences.

## Decision

- **Specifications in TOML.** A specification describes one command:
  - its name, aliases and description;
  - its options: their spellings, a description, the value they take, and whether they also apply after subcommands;
  - its subcommands, nested to any depth up to a limit;
  - what its positional arguments are.

  An argument is a list of values, or one of a few kinds: `path`, `file`, `directory`, `text`, `git-branch`, `git-tag`, `git-ref`, `git-remote` and `ssh-host`. The parser lives in `woop-config` (`spec`), next to the configuration it resembles.
- **Bundled and the user's own.** Woop includes specifications written for it, for `git`, `cargo`, `rustup`, `npm`, `docker`, `kubectl`, `ssh`, `scp`, `gh` and `pip`. A test checks that each one is valid. Users can add TOML files in a `completions` directory next to `config.toml`. A file with the name of an included one replaces it. These files are checked like `config.toml`: on Unix they are refused when others can write the file or its directory, and every file must be a regular file of at most 64 KiB. They are read on a background thread when the window starts and when the configuration is reloaded. A file that cannot be used is reported, and the others still apply.
- **Bounded and clean.** A file may hold at most 20,000 names and values, and its tables and arrays may nest at most 24 levels deep. Nesting is measured before parsing, so a crafted file cannot exhaust the stack. Names must be a single word of at most 64 printable characters. Names and descriptions may not contain control, bidirectional or zero-width characters, so the list shows what is inserted.
- **Reading the command line.** For the command before the cursor (after the last `;`, `|`, `&` or parenthesis), its first word picks the specification. The name is matched with or without a directory, and with or without `.exe` and similar endings; case is ignored on Windows. The words are then walked:
  - a subcommand moves into it, but only before the first positional argument;
  - an option that takes a value skips the next word;
  - after `--`, only arguments follow.

  The word at the cursor is then completed:
  - after an option that takes a value, with that value;
  - when it starts with `-`, with options, including global ones from the commands above;
  - otherwise, with subcommands and the argument's values.

  Options are offered once each, by the spelling that matches what was typed. Long spellings are offered after `--`.
- **Values from files, never from programs.** git names are read from the repository that contains the shell's reported directory: the refs under `refs/`, `packed-refs`, and `[remote "…"]` sections of `config`. Linked working trees (a `.git` file and `commondir`) are followed, but only to local paths: a target naming a network share or a device is refused, since opening one on Windows connects to that host and offers the user's credentials. Names starting with `-` are left out, so they cannot be read as options. SSH hosts come from the `Host` lines of `~/.ssh/config`, without patterns. These reads happen on the completion thread of [ADR 0023](0023-history-and-path-completions.md), one at a time per pane. They are bounded: files at most 1 MiB, at most 10,000 names, 64 parent directories, 8 levels of ref directories and 50,000 entries visited under them. Names that are not single printable words are left out. The inserted value is quoted for the shell like a path.
- **Fallbacks.** Commands without a specification complete paths as before. The first word of a command is still left to the shell. A spot where a specification offers nothing, such as free text, hands Tab to the shell.

## Alternatives considered

- **Running programs for dynamic values** (`git branch`, `docker ps`, `kubectl get`). These would cover more, but they run code chosen by the repository or the cluster, on every Tab, and can hang or prompt. Container and cluster names are not offered for now.
- **Reading another project's specification collection.** It would cover far more commands, but its licence and format are not Woop's to adopt, and its specifications can run programs.
- **A Rust data structure instead of TOML.** It would be checked by the compiler, but users could not add their own, and it would differ from how Woop is configured.
- **Watching the `completions` directory.** Edits would apply at once, but it would need another watcher thread. Reloading the configuration already reads the directory again.

## Consequences

- Common commands complete their structure and their branches, tags, remotes and hosts with no setup and without running anything. The list shows each subcommand's and option's description.
- The bundled specifications must be maintained by hand as the commands change. They cover common use, not every option. Users can fill gaps with their own files.
- Values that only a program can provide (containers, cluster resources, package scripts) are not completed.
- Reading a repository's files is limited to listing ref names and reading two small files. A hostile repository can only make Woop show names, which are filtered and quoted like file names.
