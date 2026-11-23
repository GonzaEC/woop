# 0021. Let the configuration change Woop's shortcuts

- **Status:** Accepted
- **Date:** 2026-11-23

## Context

Woop's shortcuts for tabs, panes, blocks and the command palette were fixed per platform ([ADR 0019](0019-tabs-and-split-panes.md)). Some of them are keys that programs use (Alt+Left and Alt+Right in some shells), and the palette has commands without any shortcut. The configuration file ([ADR 0020](0020-toml-configuration.md)) is where people change Woop, so shortcuts belong there too. A shortcut that took a key used for typing would break the terminal for whoever set it.

## Decision

- **One list of actions.** `woop_config::Action` lists the actions that can have a shortcut: the command palette's commands plus opening the palette. The palette lists the same actions, so there is a single source, and each action has a configuration name (`split-right`, `copy-last-command`).
- **The `[keys]` table.** It maps an action name to a shortcut such as `"Ctrl+Shift+T"`, or to `"none"` to remove the action's shortcut. Modifier names are `Ctrl`, `Alt` (or `Option`), `Shift` and `Cmd` (or `Super`, `Win`, `Meta`); keys are a character, a function key or a named key (`Left`, `Tab`, `PageUp`...). Unknown actions and malformed shortcuts are configuration errors, reported like any other.
- **What is accepted.** A shortcut must include Ctrl, Alt or Cmd, so that it cannot take a key used for typing. Ctrl+Alt with a character is refused, since that is AltGr on Windows and types characters on many keyboards. Other combinations are accepted, even keys a shell uses (Ctrl+R): that is the user's choice.
- **Defaults and changes.** `Bindings` in the interface starts from the platform's defaults and applies the user's changes: an action given a shortcut loses its defaults, and the shortcut is taken from any other action that had it.
- **Fixed keys.** Copying and pasting (Ctrl+C and Ctrl+V on Windows with their special rules, Ctrl+Shift+C and Ctrl+Shift+V on Linux, Cmd+C and Cmd+V on macOS) and Shift+Page Up/Down are checked first and cannot be removed; copy and paste can be given extra shortcuts.
- **Applied live.** New shortcuts apply when the configuration reloads, and the palette shows the shortcuts in effect.

## Alternatives considered

- **Shortcuts as arbitrary strings with no validation.** Simplest, but a single entry such as `"T"` would make it impossible to type that letter.
- **Refusing keys that shells use.** Safer for beginners, but people who never use those shell keys want them, and the list differs per shell.
- **Making copy and paste configurable too.** Possible, but their rules (copy only with a selection on Windows, interrupt otherwise) would have to be expressed in the configuration.

## Consequences

- Alt+arrows, and any other default, can be moved off keys a program needs.
- Every palette command can have a shortcut.
- A configuration error in `[keys]` keeps the previous settings, as for the rest of the file.
- The interface must check the bindings for every key press; with a few dozen entries, this is a short scan.
