<div align="center">

# Woop

**A fast, lightweight, GPU-accelerated terminal.**

[![CI](https://github.com/GonzaEC/woop/actions/workflows/ci.yml/badge.svg)](https://github.com/GonzaEC/woop/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

> [!NOTE]
> Woop is in early development and not yet ready for daily use. See the [roadmap](docs/roadmap.md) for current progress.

## Why Woop

- **Fast.** Native Rust with GPU rendering. Starts instantly and stays responsive with large outputs.
- **Lightweight.** A small binary with a low memory footprint. No bundled browser engine.
- **Private by default.** No account, no sign-in, no telemetry. Your terminal works offline.
- **Modern workflow.** Commands and their output are grouped into blocks, with an editor-like input, tabs, splits and a command palette.
- **Cross-platform.** Windows, macOS and Linux from day one.

## Planned features

| Feature | Description |
| --- | --- |
| Command blocks | Each command and its output form a block that can be copied, collapsed and navigated, showing exit status and duration. |
| Editor-like input | Multi-line editing with a free cursor, selection and familiar shortcuts. |
| Tabs, splits and command palette | Organise sessions and reach every action from the keyboard. |
| Completions | Suggestions from history, paths and command specifications. |
| Optional AI assistant | Bring your own API key or run a local model. Disabled unless you configure it. |
| Themes and configuration | Plain TOML configuration with live reload. |

## Building from source

Requirements:

- [Rust](https://rustup.rs) (stable, 1.85 or later)
- **Windows:** Microsoft C++ Build Tools with the "Desktop development with C++" workload
- **macOS:** Xcode Command Line Tools
- **Linux:** a C toolchain plus the development packages for your graphics stack (documented as rendering lands)

```sh
git clone https://github.com/GonzaEC/woop.git
cd woop
cargo run --release
```

## Project layout

| Path | Purpose |
| --- | --- |
| `crates/woop-app` | Application binary: window, rendering and user interface |
| `crates/woop-term` | Terminal emulation: VT parsing, screen grid and command blocks |
| `crates/woop-pty` | Cross-platform pseudo-terminal management |
| `crates/woop-config` | Configuration, themes and keybindings |
| `crates/woop-ai` | Optional AI assistant providers |
| `docs/` | Architecture, [decision records](docs/adr/README.md) and roadmap |

Read [docs/architecture.md](docs/architecture.md) for an overview of how the pieces fit together.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

Woop is released under the [MIT License](LICENSE).
