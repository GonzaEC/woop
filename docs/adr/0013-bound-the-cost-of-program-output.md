# 0013. Bound the cost and the reach of program output

- **Status:** Accepted; replies to queries completed by [0015](0015-write-input-on-a-dedicated-thread.md)
- **Date:** 2026-11-21

## Context

[ADR 0012](0012-security-tests-performance-priorities.md) makes program output untrusted input: anything a program prints, including a file shown with `cat` or output from a remote host, may be crafted by an attacker. A review of the code against that rule, followed by security and performance reviews of the fixes, found these problems:

- **Clipboard.** The emulator accepted OSC 52 by default, and Woop copied whatever it carried to the system clipboard, so displayed output could decide what the user pastes next.
- **Unbounded queue and pumps.** Output travelled from the reader thread to the window through an unbounded channel, and `Session::pump` drained it in an unbounded loop. A process writing faster than Woop parses could grow memory without limit or keep the window from ever drawing.
- **Quadratic block tracking.** Every shell integration mark walked the whole scrollback ([ADR 0011](0011-shell-integration-and-command-blocks.md)): 100,000 marks took 37 seconds in a release build. Every history trim also moved every block, a pass over up to 10,000 blocks per kilobyte of output.
- **Unnoticed history clears.** A clear (`ESC c`, `CSI 3 J`) followed by enough output in the same read went unnoticed, leaving block positions out of step with the grid; a later trim could subtract past zero.
- **Unbounded sequences and titles.** The emulator buffers OSC payloads without limit, so an unterminated one grows memory for as long as output continues. Window titles of any length, with control characters, reached the operating system.

## Decision

- **No clipboard access for programs.** OSC 52 reads and writes are disabled. If a future setting allows them, it is opt-in.
- **Back-pressure.** Output waits in a queue of at most 1 MiB (32 reads of up to 32 KiB). When the queue is full, the reader stops reading and the process blocks on its own writes. `Session::pump` stops after applying 128 KiB; if more is waiting, it notifies the owner again, so the window keeps drawing during a flood.
- **Bounded work per mark.** The number of logical lines in the scrollback is kept incrementally: rows are counted once, as they scroll into history. Anchoring a mark therefore walks the visible screen plus at most 256 wrapped rows; a mark deeper inside a wrapped line is ignored, because a shell never prints its prompt there. Command text is read by walking at most 100 rows up from the cursor. Old blocks are forgotten in batches of 1,000.
- **Batched trimming.** The scrollback may grow 400 lines past 10,000 before Woop trims it back, so block positions are adjusted once per batch instead of once per kilobyte. The batch stays below the emulator's cache of 1,000 spare rows; larger trims make it rotate and reallocate the whole scrollback (a slack of 2,048 was measured 22 to 68% slower and used 30% more memory). The guarantee of ADR 0011 is unchanged: Woop always knows how many lines it removed.
- **Clears seen in the stream.** The mark scanner also recognises `ESC c` and `CSI 3 J` and forgets the blocks at that exact point. It reads sequences exactly as the emulator's parser does (control characters, DEL and high bytes inside a sequence do not end it; only the first parameter of `CSI J` counts, as a number), so no variant slips past. Marks that would point before their block's previous mark are ignored, and a trim keeps a block only if all of its positions survive.
- **No held-back output.** Synchronized updates (mode 2026) make the parser buffer output until the program ends them, which a hostile program never does: the screen would freeze, and the scanner would see clears and marks before the emulator applies them. Woop draws once per pump anyway, so it ends every synchronized update immediately.
- **Bounded sequences and titles.** An OSC longer than 64 KiB is cancelled by feeding CAN to the emulator. Titles are limited to 256 characters, without control characters or invisible formatting characters (such as bidirectional overrides).
- **Private files.** On Unix, the shell integration directory and files are created private to the user (modes 700 and 600), and existing ones are fixed.
- **Tests.** Each problem has a regression test that fails without its fix.

## Alternatives considered

- **Rate-limiting marks.** Simpler, but it drops legitimate marks during bursts, and the cost per mark would still be linear.
- **Caching row positions of anchors.** Fast lookups, but the cache becomes invalid on every reflow, including reflows hidden behind the alternate screen.
- **Dropping output when the queue is full.** Keeps the process running, but corrupts the screen. Blocking the writer is what any terminal with a slow reader does.
- **A time budget per pump.** Bounds latency exactly, but a single chunk can still take long, and a byte budget is deterministic and testable.

## Consequences

- A flood of output or marks degrades throughput but cannot freeze the window for long or exhaust memory.
- Programs that rely on OSC 52 to copy text (some editors over SSH) cannot do so until an opt-in setting exists. OSC sequences longer than 64 KiB, such as inline images, are not supported: the first 64 KiB are handled as a complete sequence and the rest is shown as text.
- Synchronized updates are not honoured; full-screen programs that use them may show an intermediate frame at most once per pump.
- The mark scanner must keep mirroring the emulator's parser. Upgrading the emulation library requires checking that its escape sequence parsing did not change, and the regression tests cover the known variants.
- The scrollback holds between 10,000 and 10,400 lines.
- Converting an anchor in the scrollback (rather than on the screen) back to a row still walks from the top of the history. This only happens when blocks are drawn; the interface must not do it for every block on every frame.
- Replies to queries are still written on the caller's thread; volume limits for them are tracked in the roadmap.
