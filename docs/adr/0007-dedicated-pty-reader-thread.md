# 0007. Read process output on a dedicated thread

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Reading from a pseudo-terminal blocks until the process produces output. If the interface thread performed these reads, a quiet process would freeze the interface and a noisy one would starve rendering. The output must also be applied to the terminal state in order, and some sequences require an immediate reply to the process (for example, ConPTY waits for a cursor position report at startup).

## Decision

Each `Session` starts one dedicated thread that performs blocking reads and sends raw output chunks over a standard library channel. The owner of the session applies pending output to its `Terminal` by calling `Session::pump`, which also writes required replies back to the process. The `Terminal` itself performs no I/O.

## Alternatives considered

- **An async runtime (such as Tokio).** Adds a large dependency and complexity for what is one blocking reader per session; pseudo-terminal handles also lack uniform async support across platforms.
- **Sharing the terminal behind a mutex between the reader and the interface.** Removes a copy, but introduces lock contention on the render path and makes ordering bugs harder to reason about.
- **Non-blocking polling from the interface thread.** Wastes CPU when idle and adds latency when busy.

## Consequences

- The interface never blocks on process I/O, and the emulation state has a single owner.
- `Terminal` stays pure and fully unit-testable.
- Output is copied once per chunk into the channel; this is negligible compared to parsing, and can be revisited with measurements.
- The interface layer must wake up when output arrives; this will be connected to iced's subscription mechanism in the rendering milestone.
