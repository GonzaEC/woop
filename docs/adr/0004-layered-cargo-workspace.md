# 0004. Layered Cargo workspace

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

A terminal mixes very different concerns: operating-system process handling, escape sequence parsing, rendering, configuration and optional network features. Mixing them in one crate makes the emulation hard to test without a window, slows compilation and lets UI code depend on internals it should not touch.

## Decision

We will organise the code as a Cargo workspace of focused crates with dependencies flowing in one direction:

| Crate | Responsibility | May depend on |
| --- | --- | --- |
| `woop-pty` | Pseudo-terminals and child processes | nothing in the workspace |
| `woop-term` | Terminal emulation and sessions | `woop-pty` |
| `woop-config` | Configuration, themes, keybindings | nothing in the workspace |
| `woop-ai` | Optional assistant providers | `woop-config` |
| `woop-app` | Window, rendering, interface | all of the above |

Shared settings (version, edition, lints, dependency versions) are declared once at the workspace level.

## Alternatives considered

- **A single crate with modules.** Simpler at first, but boundaries are not enforced by the compiler and everything recompiles on every change.
- **Many fine-grained crates.** Stronger isolation, but more boilerplate and harder navigation than the project needs at this stage.

## Consequences

- The emulation core is testable without a window or GPU.
- The compiler enforces the layering: a lower crate cannot reach into a higher one.
- Incremental builds recompile only affected crates.
- New crates need a justification; when one is added or a boundary moves, this record is superseded.
