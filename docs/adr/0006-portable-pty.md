# 0006. portable-pty for pseudo-terminals

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Shells and interactive programs only behave as in a terminal when attached to a pseudo-terminal. Each platform exposes this differently: ConPTY on Windows, `openpty` and related calls on Unix. Handling both correctly involves process creation, handle inheritance, resizing and shutdown, each with platform-specific pitfalls.

## Decision

We will use the [`portable-pty`](https://crates.io/crates/portable-pty) crate, wrapped by `woop-pty`, which exposes a small API of its own: `Pty`, `PtySize`, `ShellCommand` and `ExitStatus`. Errors are converted to a `PtyError` type so that library details do not leak into other crates.

## Alternatives considered

- **Direct use of ConPTY and Unix PTY APIs.** Full control, but significant unsafe, platform-specific code to write and maintain.
- **The pseudo-terminal module of the emulation library.** Available, but couples process handling to the emulation dependency and its event loop, against the layering in [ADR 0004](0004-layered-cargo-workspace.md).

## Consequences

- One code path for all platforms, with no `unsafe` in Woop's own code.
- `woop-pty` stays small and replaceable behind its own types.
- Platform-specific behaviour (such as ConPTY querying the cursor position at startup) must still be handled by the session layer and covered by end-to-end tests on every platform.
