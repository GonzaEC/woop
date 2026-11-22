# 0017. Decide the command editor's keys when they are handled, and bound what it holds

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

The milestone 4 audit of the command editor ([ADR 0016](0016-command-editor-at-the-prompt.md)) found that some of its rules did not hold in practice:

- Keys were interpreted by the editor widget with the state captured when the window was last drawn. Keys typed just after Enter, before the next redraw, were partly dropped (Backspace, Enter, Ctrl+C) while letters reached the shell, so the shell could receive a different stream than the one typed.
- The widget receives key presses even without the keyboard focus, so after a click outside its text some keys acted on it and others were lost.
- Control keys other than the ones the editor uses were sent to the shell. Several of them put text into the shell's line (recalling history, yanking), which then ran together with the editor's text, and Ctrl+M or Ctrl+J ran it at once.
- AltGr arrives as Ctrl+Alt on Windows, so characters such as `ą`, `ć` or `ź` triggered the editor's Ctrl+A, Ctrl+C and Ctrl+X shortcuts instead of being typed.
- Laying out a very long line costs time on every key: after pasting 1 MiB without line breaks, each key took over half a second.

## Decision

- **Keys are decided when handled.** The editor widget only passes key presses on; the application decides what each one means with its state at that moment. A key that arrives once the editor is no longer active is handled as on the grid, so the shell receives exactly what was typed, in order. Keys reach the editor whether or not it has the focus, and any key or click on the panel gives the focus back.
- **Only keys that keep the shell's line empty reach the shell from the editor:** Ctrl+L (clear the screen), Ctrl+C and Ctrl+D from an empty editor. Ctrl+I acts as Tab (hand-off), Ctrl+M and Ctrl+J as Enter. Other control and function keys hand the prompt to the shell's own line editor when the editor is empty, and are ignored when it holds text.
- **AltGr types.** Ctrl+Alt with a printable character inserts that character.
- **The panel's states.** A prompt whose command was submitted shows as running; a prompt handed off, or whose line already held text, shows as belonging to the shell's line editor. A refused Enter cannot carry over to another prompt. The panel appears at the first prompt the editor takes, and grows with the text but shrinks only at a new prompt.
- **Bounded text.** The editor holds at most 64 KiB, with lines of at most 4 KiB. A larger paste goes to the shell as a paste when the editor is empty, and is refused with a notice otherwise.
- **Platform conventions.** While the editor is active the shell does not receive keys, so the editor follows text-field conventions on Windows and Linux (Ctrl+A, Ctrl+X, Ctrl+V; Ctrl+C copies a selection) and Cmd on macOS.

## Alternatives considered

- **Keeping the widget's own key handling and redrawing sooner.** Narrows the window after Enter but cannot close it: several key presses can be delivered before any redraw.
- **Forwarding every control key, relying on the Enter confirmation.** The confirmation depends on the shell's line staying empty; keys that fill it would also defeat it (moving the shell's cursor back to the input start makes the line look empty).
- **Refusing all large pastes.** Simpler, but large pastes into an empty prompt worked before the editor existed and still have a safe path.

## Consequences

- What the shell receives matches what was typed, also around Enter; the Enter confirmation keeps its meaning.
- Shell shortcuts on control keys (history recall, kill and yank) work only from an empty editor, by handing the prompt to the shell.
- The application implements the editor's key bindings itself, from the widget's binding list.
- Long single lines (over 4 KiB) cannot be edited in the editor.
