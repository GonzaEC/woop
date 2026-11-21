# 0005. Reuse an existing terminal emulation library

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Correct terminal emulation is a large, detail-heavy problem: hundreds of escape sequences, alternate screens, scroll regions, wide and combining characters, line reflow on resize, and the quirks real programs depend on. Getting it wrong breaks everyday tools such as editors, pagers and progress bars. This is not where Woop aims to differentiate.

## Decision

We will use the [`alacritty_terminal`](https://crates.io/crates/alacritty_terminal) crate for escape sequence parsing, the screen grid and scrollback. `woop-term` wraps it behind its own types (`Terminal`, `TerminalSize`, `TerminalEvent`) so that the rest of Woop never depends on the library directly.

## Alternatives considered

- **`vte` parser with an in-house grid.** Full ownership of the screen model, but months of work to reach the same correctness, with a long tail of compatibility bugs.
- **Writing the parser and grid from scratch.** The highest cost and risk, with no product benefit.

## Consequences

- Woop gets a mature, extensively tested emulator from the start.
- Command blocks, the editor-like input and other features are built on top of the grid rather than inside it.
- Woop follows the library's release cycle; upgrades are isolated to `woop-term` thanks to the wrapper types.
- If the screen model ever needs changes the library cannot support, the wrapper boundary allows replacing it; that would be recorded in a new ADR.
