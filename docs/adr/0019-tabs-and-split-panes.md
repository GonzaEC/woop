# 0019. Organise sessions in tabs of split panes

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

Until now a Woop window ran one shell. Milestone 5 adds tabs and splits, so several sessions share a window. Each session already owns its reader and writer threads ([ADR 0007](0007-dedicated-pty-reader-thread.md), [ADR 0015](0015-write-input-on-a-dedicated-thread.md)) and is woken by its own output ([ADR 0010](0010-screen-snapshots-and-wakeups.md)). The window must route keys, mouse, clipboard and output to the right one, keep the command editor ([ADR 0016](0016-command-editor-at-the-prompt.md)) working per session, and never let a session miss the mark token ([ADR 0018](0018-per-session-mark-token.md)).

## Decision

- **Panes and tabs.** A pane is one terminal: a session, its grid, its command editor, its history walk and its state. The interface keeps one `Pane` type per session, with its own messages, and the window maps them by a stable pane id. A tab holds the layout of its panes in iced's pane grid (`PaneGrid`, part of the existing dependency), which provides nested splits, resizing by dragging and adjacency for moving the focus. The command history is shared by all panes of the window.
- **Focus.** One pane of the shown tab has the focus. Only it reacts to keys: every other grid ignores key presses and every other editor stays inactive. Clicking a pane focuses it, also with the right button that pastes. The interface handles all the events of a frame before drawing again, so keys pressed in the same frame as a focus change are read by the pane that had the focus; the window therefore drops keyboard input (keys, editor keys, pastes) from any pane that no longer has the focus when it is handled, rather than send it to a shell the user stopped looking at.
- **Output.** Each session's notifier sends its pane id over one channel. Panes in hidden tabs keep reading and applying output, so their state is current when shown; they are only not drawn.
- **Sessions start signed.** `shell_integration::spawn` generates the token, applies the integration, starts the session and requires the token before returning it, so no caller can forget it (closing a low finding of the token review).
- **Shortcuts.** These keys are not used by the shells, and are left to the workspace also while the command editor is active:

  | Action | Windows and Linux | macOS |
  | --- | --- | --- |
  | New tab | Ctrl+Shift+T | Cmd+T |
  | Close pane (the tab closes with its last pane) | Ctrl+Shift+W | Cmd+W |
  | Next and previous tab | Ctrl+Tab, Ctrl+Shift+Tab | the same |
  | Split right | Ctrl+Shift+D | Cmd+D |
  | Split down | Ctrl+Shift+E | Cmd+Shift+D |
  | Move the focus | Alt+arrows | Cmd+Option+arrows |

  Option+arrows moves by words on macOS, hence the extra modifier there.
- **Closing.** Closing a pane whose command is still running asks for confirmation. The terminal tracks a running command from the shell's signed marks (`Terminal::command_running`), apart from the blocks, which output can make Woop forget (clearing the history, or scrolling the command out of it). Without shell integration Woop cannot tell, so closing always asks: the first press shows a message in the window, and pressing the shortcut again closes it. A pane closes on its own when its shell exits. Closing the last pane of a tab closes the tab, and closing the last tab closes the window. A closed session's process is ended.
- **New panes.** New panes start the default shell in the default directory, at the size of the focused pane until their own layout is known.

## Alternatives considered

- **One command editor for the window, writing to the focused pane.** Saves space, but its state (text, confirmation, hand-off) belongs to one session and would have to move with the focus; per pane is simpler to reason about.
- **Writing our own split layout.** Possible, but the pane grid already handles nesting, dragging and adjacency, and is maintained with the interface library.
- **Pausing hidden sessions.** Would save work, but a process blocks once its output is not read ([ADR 0007](0007-dedicated-pty-reader-thread.md)); hidden panes must keep reading.
- **Closing without confirmation.** Simpler, but ends a running command (a build, an editor with unsaved work) on a single key.

## Consequences

- Each pane costs its own threads, pseudo-terminal and scrollback; memory grows with the number of panes.
- Every message of a pane carries its id, and key events reach all panes' widgets, which must check whether they have the focus.
- New panes do not inherit the working directory of the focused pane; that needs the shell to report its directory (OSC 7) and is left for later.
- Tab titles follow the title of each tab's focused pane, shortened to the last part of a path.
- Each tab's grid keeps its own widget state (keyed by the tab), so a tab shown again measures its panes afresh.
- Alt+Left and Alt+Right are taken from programs that use them (some shells move by words or directories with them) while the command editor is not active; they can be rebound once configuration exists.
