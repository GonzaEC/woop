# 0016. Edit commands in Woop's own editor while the shell waits at a prompt

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

Until now every key went straight to the shell, so editing a command depended on the shell's line editor: no free cursor placed with the mouse, no selection with Shift and the mouse, and multi-line commands that are awkward to write. Milestone 4 adds an editor owned by Woop. Woop only knows that the shell is waiting for a command through the shell integration marks ([ADR 0011](0011-shell-integration-and-command-blocks.md)); full-screen applications, programs reading their own input (password prompts, `read`) and shells without integration must keep receiving keys exactly as before. Whatever the editor sends is input to the shell, so it must never carry anything the user did not type ([ADR 0012](0012-security-tests-performance-priorities.md)).

## Decision

- **When the editor is active.** `Terminal::screen()` reports the block waiting for a command (`Screen::awaiting_command`) when the last block received its command start mark (OSC 133 `B`) but no output start, the alternate screen is off, and the cursor is still exactly where the command input starts, that is, the shell's own line is empty (text typed ahead while the previous command ran is left to the shell). The check walks the screen rows only. In every other state keys go to the shell as before. Once on, the editor stays on for that prompt even if output moves the cursor (a background job printing a line), because switching keys back to the shell halfway through a command would send its end on its own; it turns off when the command is submitted or handed off, when output starts, at the next prompt and on the alternate screen. If, when Enter is pressed, the shell's line can no longer be shown to be empty, nothing is sent and the panel asks for a second Enter.
- **Where it is drawn.** In a panel under the terminal grid, using the text editor widget of iced (already a dependency, [ADR 0003](0003-native-gpu-ui-with-iced.md)). The panel appears once the shell reports its first mark and then stays, disabled while a command runs, so that the grid is not resized on every command; it is hidden on the alternate screen. It grows with the text up to 8 lines and shrinks back at the next prompt, never while a command starts.
- **Keys.** The editor handles text, cursor movement, selection (Shift with movement keys, mouse, Ctrl+A / Cmd+A), copy, cut, paste and line breaks (Shift+Enter). Enter submits. Up on the first line and Down on the last line walk Woop's history. Ctrl+C copies the editor's selection, else the grid's selection, else clears the editor, and on an empty editor reaches the shell as an interrupt. Woop's own shortcuts (jumping between blocks, scrolling pages) keep working. Other keys the editor does not use (such as Ctrl+L, or Ctrl+D with an empty editor) are sent to the shell as typed; its line is empty, so they act as they would at an empty prompt.
- **Handing off to the shell.** Tab and Ctrl+R type the editor's text into the shell, followed by the key, and leave that prompt to the shell's own line editor, so its completion and history search keep working until milestone 6 replaces them. Multi-line text is never handed off (the key is ignored): a line break would run part of the command unless the shell treats it as a paste, and the bracketed paste mode cannot prove that, because program output can turn it on behind a shell that does not understand it.
- **What is sent on Enter.** `input::encode_command`: the text with control characters removed, as for a paste, wrapped in bracketed paste markers when the shell asked for them, followed by a carriage return. Shells with bracketed paste receive a multi-line command as one command; without it each line runs in turn, as when it is typed. If the input cannot be queued (the backlog of [ADR 0015](0015-write-input-on-a-dedicated-thread.md) is full), the text stays in the editor.
- **History.** Submitted commands are kept in memory only, at most 1,000 entries and 64 KiB each; consecutive duplicates are kept once, and commands starting with a space are not kept, so a secret can be typed without being remembered. Nothing is written to disk. Persisting it is decided with completions in milestone 6.

## Alternatives considered

- **Drawing the text on the prompt row of the grid.** Looks the most integrated, but the shell redraws that row (prompts that change, resizes, background jobs printing), and prompts of several rows or with right-aligned parts would have to be understood by Woop.
- **Woop's own text model and drawing.** Fully under Woop's control and testable without a window, but means writing cursor movement, selection, input methods and wrapping again; the iced widget provides them and Woop tests its own logic (routing keys, history, encoding) separately.
- **Ignoring Tab and Ctrl+R until milestone 6.** Simpler, but removes completion, which users rely on, for several milestones.
- **History on disk.** More useful across sessions, but writes everything typed, including secrets, to a file; deferred until it can be designed with its protections.
- **Sending the text as individual key presses.** Would let the shell's line editor see every key, but key sequences differ per shell and line breaks would run lines early.

## Consequences

- Commands can be written and changed like text in an editor at any prompt of a shell with Woop's integration, without setup. Shells without it, full-screen applications and programs reading their own input are unaffected.
- The panel takes a row or more from the grid once shell integration is active.
- The editor relies on the OSC 133 marks. Output can forge them (a low finding of the milestone 3 audit), which would make the editor appear while another program reads its input; what the user types there is then still only sent when they press Enter. Marks carrying a per-session token remove this and become more valuable with the editor.
- Two line editors exist side by side: the shell's still handles completion and history search after a hand-off, and its own history does not include commands typed in Woop's editor unless the shell records executed commands (most do).
- Ghost text and syntax colouring drawn by the shell's line editor are not shown while Woop's editor is active.
