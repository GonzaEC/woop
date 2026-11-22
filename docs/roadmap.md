# Roadmap

Each milestone produces something that can be built, run and tested, and is closed with a security and performance audit of what it added (see "Reviews" in `AGENTS.md`).

## Quality track

Security, tests and performance come before new features ([ADR 0012](adr/0012-security-tests-performance-priorities.md)). This work runs alongside the milestones and takes precedence over them.

- [x] **Security baseline.** `unsafe` forbidden across the workspace; dependency audit in CI (known vulnerabilities, licences, sources); automated dependency updates; read-only CI permissions.
- [x] **Security hardening.** Programs cannot use the clipboard (OSC 52); bounded queue and bounded work per pump between the output reader and the window; shell integration files private to their owner; block tracking that stays cheap under a flood of marks ([ADR 0013](adr/0013-bound-the-cost-of-program-output.md)).
- [x] **Robustness tests.** Property-based tests showing that the emulator, the mark scanner and the input encoders never panic or hang on arbitrary input ([ADR 0014](adr/0014-property-based-robustness-tests.md)).
- [x] **Replies off the interface thread.** Input, paste and replies to terminal queries are written by a dedicated thread with a bounded backlog, and replies are limited per pump, so a process that does not read its input cannot stall the window ([ADR 0015](adr/0015-write-input-on-a-dedicated-thread.md)).
- [ ] **Benchmarks.** Measured throughput of parsing and snapshotting, startup time and memory use in release builds, tracked over time.
- [ ] **Hardening from the milestone 3 audit** (low findings):
  - Only trust OSC 133 marks carrying a per-session token known to the integration scripts, so displayed files and remote hosts cannot forge blocks or exit codes.
  - Integration files: reject a relative `XDG_CACHE_HOME`, check that the directory and its parents belong to the user and are not writable by others, and open files without following symbolic links.
  - Ask before pasting several lines when the application has not enabled bracketed paste.
  - Selecting text on a fold summary selects the hidden first line; make summaries unselectable or select the whole fold.
  - Alt+click is a window-dragging gesture on many Linux desktops; offer another way to copy a block's command (command palette or context menu).
  - bash 5.1+: elements of an array `PROMPT_COMMAND` after the first run after Woop's prompt hook.
  - End-to-end tests for fish and for PowerShell 7 with a profile that defines its own prompt.
  - Cache fold positions between snapshots, and walk only the view in `visible()`.

## Milestones

- [x] **0. Foundation.** Cargo workspace, CI on Windows, macOS and Linux, contribution guidelines.
- [x] **1. Headless core.** Spawn a shell through a PTY and parse its output into a screen grid, with tests.
- [x] **2. Classic terminal.**
  - [x] Window, GPU rendering of the grid, colours and text attributes, cursor.
  - [x] Keyboard input, paste, scrollback, resize.
  - [x] Mouse selection (drag, double click for words, triple click for lines, Alt+drag for blocks) and copy.
- [x] **3. Command blocks.**
  - [x] Shell integration for PowerShell, bash, zsh and fish; commands tracked as blocks with exit status and duration.
  - [x] Blocks in the interface: a coloured bar per block (running, succeeded, failed), separators, exit code and duration, jump between blocks (Ctrl+Shift+Up/Down, Cmd+Up/Down on macOS), click the bar to select a block's output.
  - [x] Collapse a block's output (double-click its bar) and copy a block's command (Alt+click its bar).
  - [x] Right click pastes the clipboard.
  - [x] Milestone audit: security, performance and general reviews, long robustness runs. Fixed: copied commands now match what is displayed and cost bounded work per mark; blocks are identified by stable ids; folds near the top of the scrollback, scrolling and jumping with folds; per-frame cost with many collapsed blocks; bash keeps the exit status for the user's `PROMPT_COMMAND` and leaves a user's DEBUG trap alone. Low findings are listed in the quality track.
- [ ] **4. Editor-like input.** Multi-line input with a free cursor, selection and editing shortcuts.
- [ ] **5. Workspace.** Tabs, splits, command palette, TOML configuration with live reload, themes.
- [ ] **6. Completions and assistant.** History and path completions, command specifications, optional AI providers (own API key or local model).
- [ ] **7. Distribution.** Signed installers and packages for each platform, published from CI.
