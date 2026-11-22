# 0014. Test untrusted input with property-based robustness tests

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

[ADR 0012](0012-security-tests-performance-priorities.md) requires that code handling untrusted input never panics or hangs on arbitrary bytes, and proves it with tests. Hand-written tests only cover the cases someone thought of. Three rounds of security review on [ADR 0013](0013-bound-the-cost-of-program-output.md) kept finding new byte sequences that broke assumptions, which shows that the input space is too large to cover by hand.

## Decision

- **Properties, not examples.** `crates/woop-term/tests/robustness.rs` states properties that must hold for any input and checks them with [proptest](https://crates.io/crates/proptest), which generates inputs and shrinks failures to a minimal example:
  - any sequence of output, resizes, scrolling and selection keeps the terminal consistent (no panic, valid screen, cursor and block positions);
  - splitting output into reads at any points changes nothing: the screen, the history and the blocks are identical, so the mark scanner and the emulator stay in step;
  - after any garbage, a reset brings the terminal back;
  - pasted text cannot act as keys or end a bracketed paste early;
  - every key encodes without panicking.
- **Inputs built from what matters.** Generated output mixes plain text with the tokens the parser and scanner care about (escape sequences, marks, clears, control characters, split and invalid UTF-8, wide and combining characters), so random cases reach deep states quickly.
- **Cases.** CI runs 256 cases per property. `PROPTEST_CASES` raises the count for longer searches, which are run before merging changes to parsing code, in both debug builds (which also check the dependencies' internal assertions) and release builds.
- **Dependency.** proptest is a development dependency only, without default features; it is never part of the application.

## Findings

The first runs found five defects, each now fixed with a regression test:

- **Wide characters in a one-column grid** made the emulator panic. A window shrunk to its minimum produced that grid. Terminals now have at least two columns.
- **The view could scroll past the history.** Output arriving while the view was scrolled back could leave the emulator's view offset larger than its history, so drawing read rows that do not exist. This is a panic in debug builds and wrong rows in release. Woop now clamps the view after every piece of output.
- **Characters split between reads.** The parser can skip up to two bytes, an ESC among them, after a character split between two calls. That changes what is shown and puts the parser out of step with the mark scanner. Woop now keeps the incomplete character at the end of a read for the next one, and its internal pieces end on character boundaries. Whatever the parser may still buffer (an invalid sequence before the cut) is then always followed by a lead byte or an ESC, which it handles exactly like unsplit input.
- **Pasted control characters** reached the shell as keys when the application had not enabled bracketed paste. Text copied from a web page could hide Ctrl+C, Ctrl+U or escape sequences. Pasted text now keeps only printable characters, tabs and line breaks.
- **The minimum terminal size was not applied** to sizes built directly from their fields; the terminal and the pseudo-terminal now use the same clamped size.

The second and third defects are in the emulation library's dependencies (its grid and its parser); Woop works around them and they should be reported upstream.

## Alternatives considered

- **Coverage-guided fuzzing (cargo-fuzz).** Finds deeper bugs, but needs a nightly toolchain and long runs outside the normal test suite. It remains an option for later; the properties here run in every CI build.
- **A hand-written random generator.** Avoids a dependency, but loses shrinking: failures would be large random inputs that are hard to debug.

## Consequences

- New code that handles output, input or paste adds or extends a property in `tests/robustness.rs`, not only example tests.
- Upgrading the emulation library is checked with long property runs, since its bugs surface through these tests.
- A property failure prints a minimal input; it becomes a regression test before it is fixed.
