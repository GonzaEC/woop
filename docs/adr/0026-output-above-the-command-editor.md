# 0026. Draw the output just above the command editor

- **Status:** Accepted
- **Date:** 2026-11-26

## Context

The command editor ([ADR 0016](0016-command-editor-at-the-prompt.md)) is a panel under the grid. A terminal fills its screen from the top, so after a few commands, or after clearing the screen, the prompt sits near the top. The editor where the user types is at the bottom, with an empty screen between them. The editor's small, faint marker and a thin line above it did not make it read as the input of the current block either. These were the main points the interface design milestone had to settle.

Three layouts were drawn and compared:
- output aligned to the bottom, just above the editor;
- the editor following the prompt down the screen;
- the current layout, styled more clearly.

## Decision

- **Bottom-aligned output.** While the shell integration's panel is shown, the screen is drawn lower by the number of unused rows at its bottom, so that its last used row sits just above the editor:
  - The used rows (`view::used_rows`) end at the last row with text, a coloured background or a selection, or at the cursor, whichever is lower.
  - They are worked out once per snapshot, walking up from the bottom.
  - Once the screen is full, nothing moves.
  - Full-screen applications, and shells without integration, are drawn from the top as before.
- **Only drawing moves.** The emulator, the program's cursor positions and the grid size do not change. The same shift turns clicks back into cells, so selection, block bars and double clicks work as before. A click above the first row selects from row 0.
- **The editor belongs to the current block.**
  - The waiting prompt gets a bar in the editor's colour, in the same place as the other blocks' bars, and the bar runs to the bottom of the grid.
  - The editor's panel continues it down its left edge, on a raised background (`theme::raised`) instead of a line.
  - The marker is a heavier arrow (❯).
  - In panes without the focus, both the bar and the marker are dim.

## Alternatives considered

- **Moving the editor under the prompt.** The panel would follow the prompt down the screen, and the empty space would end up under it instead of above. It would need the panel laid over the grid's rows, which then could not show output, or a grid resized on every prompt.
- **Keeping the layout and only styling the panel.** This is the cheapest option, but it leaves the gap between prompt and editor, which was the main complaint.

## Consequences

- The prompt and the editor are always next to each other. New output pushes older output up, so the screen keeps its newest content at the bottom while it is not full.
- After clearing the screen, the prompt appears at the bottom instead of the top. This happens only while the editor's panel is shown.
- Every snapshot walks up from the bottom to its last used row, which is a walk over at most the screen, as other per-snapshot work does. Drawing is unchanged.
