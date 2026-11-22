# Roadmap

Each milestone produces something that can be built, run and tested.

## Quality track

Security, tests and performance come before new features ([ADR 0012](adr/0012-security-tests-performance-priorities.md)). This work runs alongside the milestones and takes precedence over them.

- [x] **Security baseline.** `unsafe` forbidden across the workspace; dependency audit in CI (known vulnerabilities, licences, sources); automated dependency updates; read-only CI permissions.
- [x] **Security hardening.** Programs cannot use the clipboard (OSC 52); bounded queue and bounded work per pump between the output reader and the window; shell integration files private to their owner; block tracking that stays cheap under a flood of marks ([ADR 0013](adr/0013-bound-the-cost-of-program-output.md)).
- [ ] **Robustness tests.** Property-based tests showing that the emulator, the mark scanner and the input encoders never panic or hang on arbitrary input.
- [ ] **Replies off the interface thread.** Answers to terminal queries are written synchronously when output is pumped; limit their volume per pump or write them from another thread, so a flood of queries to a program that does not read its input cannot stall the window.
- [ ] **Benchmarks.** Measured throughput of parsing and snapshotting, startup time and memory use in release builds, tracked over time.

## Milestones

- [x] **0. Foundation.** Cargo workspace, CI on Windows, macOS and Linux, contribution guidelines.
- [x] **1. Headless core.** Spawn a shell through a PTY and parse its output into a screen grid, with tests.
- [x] **2. Classic terminal.**
  - [x] Window, GPU rendering of the grid, colours and text attributes, cursor.
  - [x] Keyboard input, paste, scrollback, resize.
  - [x] Mouse selection (drag, double click for words, triple click for lines, Alt+drag for blocks) and copy.
- [ ] **3. Command blocks.**
  - [x] Shell integration for PowerShell, bash, zsh and fish; commands tracked as blocks with exit status and duration.
  - [ ] Blocks in the interface: separators, status and duration, copy a block's command or output, jump between blocks, collapse output.
- [ ] **4. Editor-like input.** Multi-line input with a free cursor, selection and editing shortcuts.
- [ ] **5. Workspace.** Tabs, splits, command palette, TOML configuration with live reload, themes.
- [ ] **6. Completions and assistant.** History and path completions, command specifications, optional AI providers (own API key or local model).
- [ ] **7. Distribution.** Signed installers and packages for each platform, published from CI.
