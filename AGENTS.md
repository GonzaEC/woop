# Agent guide

Instructions for AI coding agents (and a quick reference for humans) working on Woop. Read this before making changes. Human contributors should also read [CONTRIBUTING.md](CONTRIBUTING.md).

## Project

Woop is a fast, lightweight, GPU-accelerated terminal written in Rust, targeting Windows, macOS and Linux. Core principles: performance, low memory use, no accounts, no telemetry ([ADR 0008](docs/adr/0008-no-accounts-no-telemetry.md)).

- Architecture overview: [docs/architecture.md](docs/architecture.md)
- Decisions and their reasons: [docs/adr/](docs/adr/README.md)
- Milestones: [docs/roadmap.md](docs/roadmap.md)

## Commands

```sh
cargo build                                            # build everything
cargo test --workspace                                 # all tests, including end-to-end PTY tests
cargo fmt --all                                        # format
cargo clippy --workspace --all-targets -- -D warnings  # lint; CI treats warnings as errors
cargo run -p woop-term --example headless -- "echo hi" # run the core without a window
```

Run `fmt`, `clippy` and `test` before declaring any change finished. These are exactly what CI checks.

## Layout and boundaries

| Crate | Responsibility | May depend on |
| --- | --- | --- |
| `woop-pty` | Pseudo-terminals and child processes | nothing in the workspace |
| `woop-term` | Terminal emulation (`Terminal`) and sessions (`Session`) | `woop-pty` |
| `woop-config` | Configuration, themes, keybindings | nothing in the workspace |
| `woop-ai` | Optional assistant providers | `woop-config` |
| `woop-app` | Window, rendering, interface (iced) | all of the above |

- Never let a lower crate depend on a higher one ([ADR 0004](docs/adr/0004-layered-cargo-workspace.md)).
- Third-party types stay behind Woop's own wrapper types; do not expose library types in public APIs of `woop-pty` or `woop-term`.
- `Terminal` performs no I/O; process handling belongs in `Session` and `woop-pty` ([ADR 0007](docs/adr/0007-dedicated-pty-reader-thread.md)).

## Code conventions

- Stable Rust, edition 2024. No `unsafe` in Woop's own code without an ADR.
- Every public item has a doc comment. Errors use `thiserror` enums per crate; avoid `unwrap()` outside tests.
- Platform-specific code goes behind `cfg` attributes and must keep all three platforms compiling.
- Keep allocations and blocking calls off the render path.
- Add tests with every behaviour change: unit tests next to the code, end-to-end tests in `tests/`. Prefer real processes over mocks for PTY behaviour.

## Decisions

Architecturally significant changes (new structural dependency, crate boundary change, cross-cutting rule) need a new ADR in `docs/adr/`, using `docs/adr/template.md`, added to the index in the same pull request. Never edit an accepted ADR; supersede it with a new one.

## Writing style

The repository is public. Documentation, comments, commit messages and pull request descriptions must be:

- In English, clear and professional.
- About Woop on its own terms. Describe features by what they do; **never compare Woop with, or mention, other terminal applications or commercial products**. Naming a library that is an actual dependency (in `Cargo.toml` or in an ADR) is fine.

## Commits and pull requests

- Work on a branch; never push to `main`. Changes merge through squash-merged pull requests that pass CI ([ADR 0009](docs/adr/0009-pull-requests-gated-by-ci.md)).
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org): `feat(term): ...`, `fix(pty): ...`, `docs: ...`.
- Do not add tool-attribution trailers or footers (such as `Co-Authored-By` lines for AI tools, or "generated with" notes) to commits or pull requests. The person submitting a change is its author and is responsible for understanding and testing it.
- Pull request descriptions use the template: a summary of what and why, and how it was tested on which platforms.
