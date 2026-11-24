# 0023. Complete commands from the history and paths from the shell's directory

- **Status:** Accepted
- **Date:** 2026-11-23

## Context

Woop's command editor ([ADR 0016](0016-command-editor-at-the-prompt.md)) hands Tab to the shell: it types the editor's text into the shell's own line and leaves the rest of the prompt to the shell's line editor. Completion then works, but the editor is gone for that prompt. Its history holds only the commands typed in the editor since Woop started, and ADR 0016 left open whether to keep it on disk.

Completion reads two kinds of data Woop does not control:

- the shell's history file, which can hold secrets and, if others can write it, commands planted for the user to run;
- directory listings, which can hold names made to look like other names (control characters, line breaks) and which can be slow or hang on network mounts.

Whatever completion inserts becomes part of a command the user runs.

## Decision

- **Tab completes in Woop.** Tab (and Ctrl+I) in the editor asks Woop for completions of the cursor's line. With none, the text is handed to the shell as before. With exactly one, it is inserted. With several, a list opens over the bottom of the grid. If they are all paths with a longer shared beginning, that beginning is inserted first, as shells do. While the list is open, Up and Down choose an entry, Tab and Enter insert it, a click inserts the entry clicked, and Escape closes the list. Any other key closes it and acts as usual. Ctrl+R still hands off to the shell's history search.
- **Paths.** The word before the cursor is completed as a path relative to the directory the shell reported in its last signed prompt ([ADR 0022](0022-working-directory-from-signed-marks.md)). With no reported directory there are no path completions. Words are split at spaces outside quotes, and quotes and escapes are read the way the pane's shell reads them, so a word that is already quoted can be completed. `~` stands for the home directory. Names starting with `.` are offered only when the word does. The name replaces the whole word, quoted for the pane's shell when it holds anything other than letters, digits and `-._/:+` (and `\` in PowerShell, where it separates paths). PowerShell also reads the typographic quotes (U+2018 to U+201E) as quotes; inside single quotes each of its single quotes is doubled, so a name cannot end the string early. A name starting with `-` gets `./` in front, so it is not read as an option. A leading `~` stays outside the quotes, so the shell still expands it. A first word without a separator names a command, which the shell completes better, so it gets no path completions. Directories get a trailing separator: `\` for PowerShell on Windows, unless the word already uses `/`, and `/` otherwise.
- **Listing off the interface thread.** Directories are read on a thread started for each request, so a slow or hung mount never freezes the window. A result that arrives after the editor's text changed is dropped. At most 10,000 entries are read. Names that are not valid UTF-8, or that contain control characters, bidirectional or zero-width formatting or line separators, are left out: they could not be typed, or they would display as something other than what is inserted. Only one listing runs at a time in each pane; Tab meanwhile hands off to the shell. A listing that takes longer than 300 ms opens the list even for a single match, so a line the user may be about to run does not change under them.
- **History.** Commands typed in the editor stay in memory only, as before. Woop also reads the history file the pane's shell keeps by default, and never writes it:
  - bash: `~/.bash_history`;
  - zsh: `$ZDOTDIR/.zsh_history` or `~/.zsh_history`;
  - fish: `fish/fish_history` under the XDG data directory;
  - PowerShell: PSReadLine's `ConsoleHost_history.txt`.

  A history file set by the user's configuration (`HISTFILE`) is not known to Woop and is not read. The file is read once for each shell, on a background thread, when the window starts its first pane with that shell. The rules for reading it:
  - only if it is a regular file;
  - on Unix, only if others cannot write it;
  - only its last 1 MiB, and at most its last 10,000 commands;
  - each format's escapes are decoded (zsh's metafied bytes, fish's escapes, line continuations), and control characters other than line breaks are removed;
  - commands too long for the editor are left out.

  Up and Down walk the editor's commands, then the file's.
- **History completions.** When the cursor is at the end of the editor's text, commands that start with that text are offered, newest first and without repeats, after the paths, at most 10 of them. One replaces the whole text.
- **What is inserted.** Insertion only places text in the editor. Nothing runs until the user presses Enter, and the text is then sent as any other command (control characters removed, [ADR 0016](0016-command-editor-at-the-prompt.md)).

## Alternatives considered

- **Suggestions while typing, accepted with Tab.** Puts completions in view without a key, but needs a listing or a history search on every key. It also means Tab can no longer reach the shell's own completion. It can be added later on top of the same completion functions.
- **Writing Woop's own history file.** Survives restarts without depending on the shell, but writes everything typed, secrets included, to a new file. The shells already keep a history with their own rules for leaving commands out (`HISTCONTROL`, `HISTIGNORE`, a leading space). Reading theirs adds nothing new to disk.
- **Reading `HISTFILE` from the shell.** Would need the shell to report it in a mark. This can be added once a user needs it; the default locations cover most setups.
- **Listing on the interface thread with a time limit.** A listing cannot be interrupted once a call blocks, so a hung mount would still freeze the window.
- **Asking the shell for its completions.** Shells offer no common interface for it. Running the shell's completion on the hidden line would type into it, which ADR 0016 rules out.

## Consequences

- Tab completes paths and history without leaving the editor, for every shell with Woop's integration. Shells without integration are unaffected: the editor is not shown for them.
- Reading the history file puts up to 1 MiB of the user's past commands in Woop's memory, for each shell used, for as long as the window runs. It is never written, logged or sent anywhere. Future assistant providers must not receive it without the user's explicit action.
- A history file kept elsewhere is not read. Commands typed in other windows after the file was read appear only in a new Woop window.
- A thread is started for each listing, one at a time per pane. A hung mount keeps its thread blocked, and Tab in that pane then goes to the shell's own completion.
- Quoting follows each shell's rules. A mistake there inserts a different name than the one shown, so the rules are tested for each shell.
