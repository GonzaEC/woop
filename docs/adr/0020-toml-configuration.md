# 0020. Configure Woop with a TOML file that reloads while it runs

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

The font, the shell and the colours were fixed in the code. Users need to change them, and to see the result without restarting. The configuration is read from disk and one of its settings chooses the program Woop starts, so the file must be found only where the user keeps their own settings, and a broken or hostile file must neither crash Woop nor take more than a bounded effort.

## Decision

- **Location.** One file, `config.toml`, in the user's configuration directory. On Windows this is `%APPDATA%\woop`. Elsewhere it is `$XDG_CONFIG_HOME/woop`, or `~/.config/woop` when that variable is unset, empty or not an absolute path. Woop only reads it; it never creates or writes it.
- **Contents.** The first version configures the font (`family`, `size`), the shell (`program`, `args`) and the colours (`theme`, which names a built-in theme, `dark` or `light`, and overrides for `foreground`, `background`, `cursor`, `selection` and the 16 `ansi` colours as `#rrggbb`). Every setting is optional. Unknown keys are errors, so a typo is reported instead of ignored.
- **Validation and bounds.**
  - Files over 64 KiB are not read.
  - TOML nesting is limited by the parser's default depth.
  - The font size must be between 6 and 72.
  - Names, the program and each argument are at most 4,096 characters, with at most 64 arguments.
  - On Unix a file that other users can write, or that is in a directory they can write, is refused: it chooses the program Woop runs. On Windows `%APPDATA%` is assumed private to the user.
  - The file is checked and read through one open handle, must be a regular file, and is read no further than the limit.
  - Text settings may not contain control or direction-changing characters, and error messages are cleaned of them and limited in length and lines.
  - The shell program must be an absolute path or a name found in the absolute directories of `PATH`. Woop resolves it to an absolute path before starting it (in `woop-pty`, for every program), because Windows otherwise looks for a bare name in Woop's own working directory, where a program of the same name could have been planted.
- **Errors.** A file that cannot be read or is invalid leaves the last valid configuration in place, or the defaults at start-up. The window shows the error (with the line, when the parser gives one) until the file is fixed. A missing file is not an error.
- **Reload.** A thread checks the file's modification time and size every 2 seconds and reloads it when they change. The cost is two small system calls every 2 seconds and no work for the interface otherwise. Colours and the font apply at once to every pane. The shell applies to panes opened afterwards, so running sessions are never restarted.
- **Crate.** `woop-config` parses and validates the file into its own plain types (it depends on nothing in the workspace, [ADR 0004](0004-layered-cargo-workspace.md)); `woop-app` turns them into colours, fonts and the shell command. Parsing uses `toml` 1.2 (reading only, without its writer) and `serde` with `derive`, which already reaches Woop through the emulation library. `toml` and its parser forbid `unsafe` code; `winnow`, the parsing library beneath it, uses some for slicing input.

## Alternatives considered

- **Watching the file with operating system notifications.** Reloads at once, but adds a dependency with platform-specific code for a change people make by hand.
- **Reloading when the window regains focus.** No periodic work, but editing the file in a window beside Woop would not show the result.
- **Falling back to the defaults on any error.** Simpler, but one typo while editing would reset the colours and font in every pane.
- **A configuration format written by hand.** Avoids a dependency, but TOML is expected by users and its parser is well maintained and tested.

## Consequences

- People can choose their shell, font and colours, and try changes live.
- iced needs font names that live for the whole program, so each distinct family name a user configures stays in memory until Woop exits; names are bounded in length and only change when the user edits the file.
- Woop does a small check every 2 seconds even when idle.
- Keybindings are not configurable yet; they come in a later change, with the command palette.
