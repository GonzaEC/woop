# Roadmap

Each milestone produces something that can be built, run and tested, and is closed with a security and performance audit of what it added (see "Reviews" in `AGENTS.md`). Features come first; the look of the interface is designed in one pass once they are in place (milestone 7).

## Quality track

Security, tests and performance come before new features ([ADR 0012](adr/0012-security-tests-performance-priorities.md)). This work runs alongside the milestones and takes precedence over them.

- [x] **Security baseline.** `unsafe` forbidden across the workspace; dependency audit in CI (known vulnerabilities, licences, sources); automated dependency updates; read-only CI permissions.
- [x] **Security hardening.** Programs cannot use the clipboard (OSC 52); bounded queue and bounded work per pump between the output reader and the window; shell integration files private to their owner; block tracking that stays cheap under a flood of marks ([ADR 0013](adr/0013-bound-the-cost-of-program-output.md)).
- [x] **Robustness tests.** Property-based tests showing that the emulator, the mark scanner and the input encoders never panic or hang on arbitrary input ([ADR 0014](adr/0014-property-based-robustness-tests.md)).
- [x] **Replies off the interface thread.** Input, paste and replies to terminal queries are written by a dedicated thread with a bounded backlog, and replies are limited per pump, so a process that does not read its input cannot stall the window ([ADR 0015](adr/0015-write-input-on-a-dedicated-thread.md)).
- [x] **Signed shell integration marks.** Marks count only if they carry a per-session token that Woop passes to its integration scripts, so displayed files, remote hosts and background jobs cannot forge blocks or exit codes, or turn the command editor on or off ([ADR 0018](adr/0018-per-session-mark-token.md)).
- [ ] **Benchmarks.** Measured throughput of parsing and snapshotting, startup time and memory use in release builds, tracked over time.
- [ ] **Hardening from the milestone 3 audit** (low findings):
  - Integration files: reject a relative `XDG_CACHE_HOME`, check that the directory and its parents belong to the user and are not writable by others, and open files without following symbolic links.
  - Ask before pasting several lines when the application has not enabled bracketed paste.
  - Selecting text on a fold summary selects the hidden first line; make summaries unselectable or select the whole fold.
  - Alt+click is a window-dragging gesture on many Linux desktops; offer another way to copy a block's command (command palette or context menu).
  - bash 5.1+: elements of an array `PROMPT_COMMAND` after the first run after Woop's prompt hook.
  - End-to-end tests for fish and for PowerShell 7 with a profile that defines its own prompt.
  - Cache fold positions between snapshots, and walk only the view in `visible()`.
- [ ] **Hardening from the command editor review** (low findings):
  - Without bracketed paste, tabs pasted into the editor trigger the shell's completion when the text is sent; replace them with spaces in that case.
  - Output that switches to the alternate screen while the user types turns the editor off; the keys after it go to the shell on their own. Keep the editor's keys until the user acts when it turns off with text in it.
  - The panel counts logical lines, so a long wrapped line scrolls inside one row, and Up on the second row of a wrapped first line walks the history. Use the widget's visual lines.
  - While the editor is active and focused, its caret blinks by redrawing twice per second at an idle prompt; measure the cost, or draw a caret that does not need it.
  - Alt with a letter types the letter in the editor; consider ignoring it, as the shell's Alt shortcuts do not apply there.
  - `Terminal::screen` takes about 90 µs at 50x200 but 700 µs at 100x300 (three times the cells, eight times the time); find out why.
- [ ] **Hardening from the mark token review** (low findings):
  - bash and zsh keep the token in `PS1`; a configuration that exports `PS1` passes it to programs. Print the marks from the prompt hooks instead.
  - Consider taking the token's bits from the operating system directly instead of relying on `RandomState`.
- [ ] **Hardening from the tabs and splits review** (low findings):
  - `Session::spawn` still accepts unsigned marks; make the scanner reject marks until a token is set, with an explicit choice for sessions without integration.
  - Alt+Left and Alt+Right move the focus between panes and so are taken from programs that use them; make them configurable.

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
- [x] **4. Editor-like input.**
  - [x] Command editor at the prompt: multi-line text with a free cursor, mouse and keyboard selection, copy, cut and paste; Enter runs, Shift+Enter breaks the line; in-memory history on Up and Down; Tab and Ctrl+R hand off to the shell's own completion and history search ([ADR 0016](adr/0016-command-editor-at-the-prompt.md)).
  - [x] Milestone audit: security, performance and general reviews, long robustness runs. Fixed: keys are decided when handled, so keys typed just after Enter reach the shell as typed and focus is never lost; only keys that keep the shell's line empty reach it from the editor; AltGr characters are typed instead of running shortcuts; the editor's text is bounded (a long single line made every key slow); the panel shrinks only at a new prompt; clearer panel states ([ADR 0017](adr/0017-command-editor-key-handling.md)). Low findings are listed in the quality track.
- [ ] **5. Workspace.**
  - [x] Tabs and split panes: each pane runs its own shell with its own command editor; new tab, close pane (asking again when a command is running), next and previous tab, split right or down, move the focus, drag the splits ([ADR 0019](adr/0019-tabs-and-split-panes.md)).
  - [ ] Command palette.
  - [ ] TOML configuration with live reload, themes.
  - [ ] New panes start in the focused pane's directory (the shell reports it with OSC 7).
- [ ] **6. Completions and assistant.** History and path completions, command specifications, optional AI providers (own API key or local model).
- [ ] **7. Interface design.** One design pass over the whole interface once its features are in place: how the command editor relates to the prompt and the blocks (today it sits far from the prompt on a mostly empty screen, its marker is small and faint, and the panel does not read as part of the current block), blocks, tabs, panels and the default theme.
- [ ] **8. Distribution.** Signed installers and packages for each platform, published from CI.
