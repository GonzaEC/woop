# 0011. Shell integration marks and command blocks anchored to logical lines

- **Status:** Accepted; history trimming refined by [0013](0013-bound-the-cost-of-program-output.md)
- **Date:** 2026-11-21

## Context

Command blocks group each command with its output, exit status and duration. A terminal only receives a stream of bytes, so it cannot tell where a prompt ends or a command's output begins without help from the shell. The emulation library ([ADR 0005](0005-terminal-emulation-library.md)) ignores sequences it does not know, and it gives no notice when it reflows text on resize or drops old lines from the scrollback. Positions stored as grid rows would therefore drift.

## Decision

- **Marks.** Shells report their state with the OSC 133 semantic prompt sequences: `A` (prompt starts), `B` (command input starts), `C` (output starts) and `D;<code>` (command finished). A `MarkScanner` in `woop-term` finds them in the output before it reaches the emulator. The output is fed to the emulator up to each mark, then the mark is applied with the cursor at its current position. The scanner keeps its state between reads, so a mark split across two reads is still found. On Windows, ConPTY passes these sequences through unchanged.
- **Blocks.** `Terminal::blocks()` returns one `Block` per command: its command text (read from the grid between `B` and `C`), exit code, start time and duration. Empty commands and prompts abandoned without running anything do not produce blocks. Marks received on the alternate screen are ignored.
- **Logical lines.** Block positions are stored as a logical line (a row plus the rows it wraps into) counted from the top of the scrollback, and a cell offset within that line. Reflow changes how many rows a line takes but not how many lines there are, so positions survive resizing. `Terminal::block_rows()` converts them back into rows on demand.
- **History owned by Woop.** The emulator's history limit is set slightly above Woop's own limit, output is fed in pieces of at most 1,024 bytes, and after each piece Woop trims the history back to 10,000 lines itself. Because it knows how many lines it removed, it can move every block up by exactly that amount. If the history is cleared (for example by `clear`) or could have been trimmed without notice, block positions can no longer be trusted, so the blocks are forgotten.
- **Integration scripts.** Scripts for PowerShell, bash, zsh and fish live in `crates/woop-term/shell/` and are embedded in the binary. `shell_integration::apply` recognises the shell from the program name and loads its script after the user's own configuration: PowerShell with `-NoExit -EncodedCommand`, fish with `--init-command`, bash with `--rcfile` (which sources `~/.bashrc` first) and zsh by pointing `ZDOTDIR` at Woop's files, which load the user's. bash and zsh files are written to the user's cache directory. Every script wraps the existing prompt instead of replacing it.

## Alternatives considered

- **Positions as grid rows.** Simplest, but wrong after any resize that reflows text and after the emulator drops old lines.
- **A separate grid per block.** Makes blocks independent of each other, but breaks programs that redraw earlier lines (progress bars, prompts that move the cursor up) and would mean rewriting much of the emulator.
- **Extending the emulator's parser.** Would avoid scanning the stream twice, but requires forking the library; the scan is a cheap byte-by-byte state machine.
- **Asking users to edit their shell configuration.** Works, but most users would never get blocks.

## Consequences

- Blocks work in PowerShell 5.1 and 7, bash (3.2 and later), zsh and fish without any setup, and keep the user's prompt, theme and configuration.
- Shells without integration (such as `cmd.exe` or `sh`) work as in any terminal, without blocks.
- Converting between logical lines and rows walks the scrollback. This is linear in the history size (at most 10,000 rows) and only happens on marks and when blocks are drawn; it can be cached if profiling shows a need.
- A single sequence that scrolls more than 1,024 lines at once makes Woop forget the existing blocks. This is rare and only affects blocks that were already far up in the history.
- The scripts are run by real shells in the end-to-end tests, on every platform in CI.
