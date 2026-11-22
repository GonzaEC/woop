# 0012. Put security first, then tests, then performance

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

A terminal is one of the most trusted programs on a computer. It runs the user's shell, sees every command and password typed into it, holds whatever is pasted, and displays output from any source: local programs, files of unknown origin, remote machines over SSH. Output is a stream of escape sequences that can change the terminal's state, and some sequences can make the terminal act (write to the clipboard, answer queries, open links). Bugs here are not cosmetic: a terminal that can be steered by the text it displays is an attack surface.

Woop is also meant to replace a terminal people use all day, so it must not break between versions and it must feel instant. Without an explicit order, these goals get traded against features case by case.

## Decision

Every change is judged in this order. When two of them conflict, the earlier one wins, and all three come before new features.

### 1. Security

- **Program output is untrusted input.** Anything that lets output act beyond drawing text (writing the clipboard, replying to queries with data, opening links or files, changing settings) is off by default or requires an explicit user action, and is bounded in size.
- **Input from the user stays the user's.** Pasted text cannot smuggle control sequences (bracketed paste strips escapes); nothing the shell or a program prints is ever typed back without the user's action, except the standard replies needed to run (cursor position, device attributes).
- **Bounded resources.** Every buffer, channel and history fed by program output has a limit, so a misbehaving program cannot exhaust memory.
- **No `unsafe` in Woop's own code** unless an ADR justifies it.
- **Files Woop writes** (shell integration, configuration) live in the user's own directories with permissions that keep other users out.
- **Dependencies are a liability.** Each new dependency needs a reason; dependencies are checked in CI for known vulnerabilities, licences and sources, and kept up to date.
- **No network access** except the optional AI providers the user configures ([ADR 0008](0008-no-accounts-no-telemetry.md)). Secrets are never logged or written to disk in plain text.

### 2. Tests

- Every behaviour change ships with tests; every bug fix ships with a test that fails without the fix.
- Code that handles untrusted input (escape sequences, shell marks, key and paste encoding) has robustness tests with arbitrary input: it must never panic or hang.
- PTY and shell behaviour is tested with real processes on Windows, macOS and Linux in CI, and nothing merges with a failing check ([ADR 0009](0009-pull-requests-gated-by-ci.md)).

### 3. Performance

- The hot paths are reading output, parsing it and drawing the screen. They avoid per-byte and per-cell allocations, locks and blocking calls.
- When the shell is idle, Woop does no work ([ADR 0010](0010-screen-snapshots-and-wakeups.md)).
- Changes to hot paths come with measurements (benchmarks or before/after numbers in release builds); speed claims without numbers are not accepted.

## Alternatives considered

- **No explicit priorities.** Simpler, but decisions drift toward whatever is being built at the moment, usually features.
- **Performance first.** Speed is a reason people choose Woop, but a fast terminal that can be steered by its output, or that breaks on update, is worse than a slightly slower one that is safe and stable.

## Consequences

- Some conveniences arrive later or behind an opt-in (for example, letting programs write to the clipboard).
- Pull requests take longer: tests are mandatory, and hot-path changes need numbers.
- Reviews (human or automated) check security first; see the reviewer guidelines in `AGENTS.md` and `.claude/agents/reviewer.md`.
- The CI pipeline grows to include dependency checks and robustness tests, which are tracked in the roadmap.
