# 0022. Open new panes in the shell's directory, reported in signed marks

- **Status:** Accepted
- **Date:** 2026-11-23

## Context

New tabs and panes started in Woop's own working directory ([ADR 0019](0019-tabs-and-split-panes.md)), while people expect them where they are working. Only the shell knows its current directory, so it has to tell Woop. Whatever the shell prints, output from programs can print too. A forged directory is not harmless: a new pane opened inside a hostile repository can run code, for example when a prompt runs `git status` there and the repository configures a command for it.

## Decision

- **Reported with the prompt.** The integration scripts add the current directory to the prompt-start mark, which already carries the session token ([ADR 0018](0018-per-session-mark-token.md)): `133;A;woop=<token>;cwd=<directory>`. Every prompt reports it again, so it follows `cd`. A forged mark has no token and is ignored, as before.
- **Encoded.** The directory is percent-encoded (every byte other than letters, digits and `-._~/:\`), so separators, spaces and control characters cannot end or change the mark.
- **Validated.** The terminal decodes it and keeps it (`Terminal::current_directory`) only if it is valid UTF-8, contains no control characters and is an absolute path. It is checked again when used: a new pane starts there only if it is an existing directory, and otherwise in the default directory.
- **Bounded.** The mark scanner buffers OSC payloads up to 4 KiB (instead of 256 bytes), so long paths fit. A payload over the limit is not a mark at all, so the scripts leave the directory out when its encoding exceeds 3,800 bytes, and when it cannot be encoded; the prompt mark is always sent.
- **The prompt is kept.** bash and zsh write the marks into the prompt string. At every prompt they take out the marks they added last time and wrap the prompt again with the current directory, so changes others made meanwhile (a virtual environment's name) stay.
- **Scope.** PowerShell reports only file system locations (not, for example, the registry provider). bash on Windows reports POSIX-style paths (`/c/Users`), which are not absolute Windows paths, so they are not followed. A remote shell reached with `ssh` does not run Woop's integration, so the pane keeps the last local directory, which is the right place for a new local pane.

## Alternatives considered

- **The common OSC 7 sequence (`file://host/path`).** Shells and tools already emit it, but it has no field for the token, so any output could set the directory.
- **Asking the operating system for the shell's directory.** Works on some platforms (reading `/proc` on Linux), but not portably, not for Windows shells, and not for the shell's idea of the directory (a symbolic link it went through).
- **Only trusting directories under the user's home.** Would refuse legitimate places (other drives, `/srv`) without removing the need for the token.

## Consequences

- New tabs and splits open in the focused pane's directory in PowerShell, bash, zsh and fish with Woop's integration.
- Programs cannot choose where new panes open.
- Each prompt carries the encoded directory, a few hundred bytes at most in practice.
- Directories longer than the payload limit, or not valid UTF-8, are not followed.
