# 0010. Render from screen snapshots, woken by output notifications

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

The interface must draw the terminal grid, react to keyboard and mouse input, and redraw when the shell produces output. [ADR 0005](0005-terminal-emulation-library.md) requires that the emulation library stays behind Woop's own types, and [ADR 0007](0007-dedicated-pty-reader-thread.md) left open how the interface learns that output is waiting. Polling on a timer would wake the process even when idle and add latency when busy.

## Decision

- **Snapshots.** `Terminal::screen()` copies the visible cells into a `Screen`: plain Woop types for characters, colours (`Foreground`, `Background`, `Indexed`, `Rgb`), text attributes and the cursor. The interface only ever draws a `Screen`; it never sees emulator types. Colours are resolved by the interface's theme, so themes need no changes in `woop-term`.
- **Wake-ups.** `Session::spawn_with_notifier` takes a callback that the reader thread calls when output arrives. Calls are coalesced with an atomic flag: after one notification, no other is sent until the owner calls `Session::pump`. The application forwards these notifications into iced through a subscription, then pumps, takes a new snapshot and clears the canvas cache.
- **Input encoding** lives in `woop_term::input`, independent of the windowing library: the interface maps its key events to `input::Key` and `input::Modifiers`, and `woop-term` produces the bytes, honouring modes such as application cursor keys and bracketed paste.
- **Drawing** uses an iced `canvas` with a geometry cache. Backgrounds are drawn as one rectangle per run of equal colour, and text as one draw call per run of cells sharing colour and font; wide characters are drawn individually so they cannot shift the cells after them.

## Alternatives considered

- **Exposing the emulator to the interface.** Avoids the copy, but leaks library types across crates and couples the renderer to the library's grid layout.
- **Timer-based polling (for example, 60 times per second).** Simpler, but wastes CPU while idle and delays output by up to one tick.
- **Drawing every cell separately.** Simplest code, but tens of thousands of draw calls per frame on large windows.
- **A custom wgpu widget with a glyph atlas.** The fastest option, but much more code; it remains a future optimisation if measurements show the canvas is not enough.

## Consequences

- The interface is idle when the shell is idle, and wakes immediately on output.
- Each update allocates one snapshot of rows × columns cells. This is small (a 300 × 80 window is 24,000 cells) but is a candidate for reuse if profiling shows it matters.
- Key encoding and screen mapping are unit-tested without a window.
- Run-based text drawing assumes a monospaced font whose advance matches the measured cell width; fonts are therefore chosen per platform and will become configurable.
