# Contributing to Woop

Thank you for your interest in improving Woop. This guide explains how to set up the project and what we expect from contributions.

## Getting started

1. Install [Rust](https://rustup.rs). The toolchain channel is pinned in `rust-toolchain.toml` and installed automatically.
2. Install the platform build tools listed in the [README](README.md#building-from-source).
3. Fork and clone the repository, then run:

   ```sh
   cargo build
   cargo test --workspace
   ```

## Before opening a pull request

Run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Guidelines

- **Keep changes focused.** One logical change per pull request. Open an issue first for large features or design changes.
- **Respect crate boundaries.** Terminal logic belongs in `woop-term`, process handling in `woop-pty`. The UI crate should not reach into internals. See [docs/architecture.md](docs/architecture.md).
- **Performance matters.** Avoid allocations and blocking work on the render path. Include measurements when a change affects throughput or latency.
- **Cross-platform.** Changes must build and behave correctly on Windows, macOS and Linux. Platform-specific code goes behind `cfg` attributes in clearly named modules.
- **Tests.** Add tests for new behaviour, especially parsing and state transitions in `woop-term`.
- **Documentation.** Public items carry doc comments. Update the docs when behaviour changes.

## Architecture decisions

Significant technical decisions are recorded as [Architecture Decision Records](docs/adr/README.md). If your change adds a structural dependency, moves a crate boundary or sets a cross-cutting rule, include a new ADR in the same pull request. Read the existing records first: they explain why the code is shaped the way it is.

## AI-assisted contributions

AI coding assistants are welcome. The project's conventions for agents are in [AGENTS.md](AGENTS.md), which most assistants read automatically; reusable workflows for Claude Code live in `.claude/`.

Whatever tools you use, you are the author of your contribution: make sure you understand the code, have tested it, and can answer questions about it in review. Do not add tool-attribution trailers to commits.

## Commit messages

Use the [Conventional Commits](https://www.conventionalcommits.org) format:

```
feat(term): group output into command blocks
fix(pty): resize the pseudo-terminal on window resize
docs: describe the shell integration protocol
```

## Reporting issues

Use the issue templates. For security issues, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Code of conduct

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
