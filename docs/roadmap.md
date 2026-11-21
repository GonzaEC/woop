# Roadmap

Each milestone produces something that can be built, run and tested.

- [x] **0. Foundation.** Cargo workspace, CI on Windows, macOS and Linux, contribution guidelines.
- [ ] **1. Headless core.** Spawn a shell through a PTY and parse its output into a screen grid, with tests.
- [ ] **2. Classic terminal.** Window, GPU rendering of the grid, keyboard input, colours, scrollback, selection, copy and paste.
- [ ] **3. Command blocks.** Shell integration for PowerShell, bash, zsh and fish; output grouped into blocks with exit status and duration.
- [ ] **4. Editor-like input.** Multi-line input with a free cursor, selection and editing shortcuts.
- [ ] **5. Workspace.** Tabs, splits, command palette, TOML configuration with live reload, themes.
- [ ] **6. Completions and assistant.** History and path completions, command specifications, optional AI providers (own API key or local model).
- [ ] **7. Distribution.** Signed installers and packages for each platform, published from CI.
