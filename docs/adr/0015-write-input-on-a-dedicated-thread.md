# 0015. Write process input on a dedicated thread

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

[ADR 0007](0007-dedicated-pty-reader-thread.md) moved reading to a dedicated thread, but writing stayed on the owner's thread: `Session::write` (keys and paste) and the replies to terminal queries were written with blocking calls. When a process stops reading its input, the pseudo-terminal's buffer fills and every write blocks, freezing the window: writing 2 MiB to a process that never reads blocked for 3.6 seconds on Windows and indefinitely on Unix. Replies are produced by program output, so a hostile program could also flood queries and turn them into an unbounded amount of input ([ADR 0013](0013-bound-the-cost-of-program-output.md) listed this as pending).

## Decision

- **A writer thread per session.** `Pty::take_writer` hands the input stream to a thread that writes queued chunks in order. `Session::write` only queues, so it never blocks.
- **A bounded backlog.** At most 16 MiB of input may wait for the process, counting a fixed overhead per queued chunk so that many tiny writes cannot hold more memory than that. Beyond it, `Session::write` refuses the input with `PtyError::InputBacklog` instead of growing memory; nothing is sent partially.
- **A reply budget.** Each `Session::pump` sends at most 16 KiB of replies to queries. Only a prefix is kept: once a reply does not fit, the later ones are dropped too, so a program never receives answers out of order. Replies are best effort: a process that floods queries without reading the answers loses them.
- **Shutdown.** When the session closes, input still queued is discarded rather than written: another process may outlive the shell on the same pseudo-terminal and must not receive keys meant for it. Writing happens in 4 KiB pieces, so a long paste stops soon after. The writer thread ends when its queue closes or a write fails; after a failure, input is refused as closed. A write that the process never reads cannot be interrupted, so in that case the thread stays blocked until the process exits.

## Alternatives considered

- **Non-blocking writes on the owner's thread.** Avoids a thread, but needs platform-specific non-blocking I/O on the pseudo-terminal handles, which portable-pty does not expose.
- **Dropping input when the process is slow.** Avoids the backlog, but corrupts what the user typed or pasted; refusing a whole write is predictable.
- **An unbounded queue.** Simple, but lets a process that never reads grow Woop's memory with every key and reply.

## Consequences

- Typing, pasting and output processing never wait for the process.
- A paste larger than the free backlog is refused as a whole; the interface should tell the user (tracked with the editor input work).
- Each session uses two threads (reader and writer).
