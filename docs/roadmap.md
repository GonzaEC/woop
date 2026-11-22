# Roadmap

Each milestone produces something that can be built, run and tested.

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
