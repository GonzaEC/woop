# 0018. Trust only shell integration marks carrying a per-session token

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

Command blocks ([ADR 0011](0011-shell-integration-and-command-blocks.md)) and the command editor ([ADR 0016](0016-command-editor-at-the-prompt.md)) rely on OSC 133 marks. Any output can contain them: a file shown with `cat`, a remote host reached over SSH, a background job. The audits of milestones 3 and 4 listed what forged marks can do:

- create fake blocks and exit codes;
- turn the command editor on while another program reads its input, such as a password prompt, so that what is typed is shown in the panel and kept in the history;
- turn the editor off while the user types, so the rest of a command reaches the shell on its own.

The marks come from Woop's own integration scripts, which run in the user's shell. A secret shared between Woop and those scripts lets Woop tell them apart from output that merely contains the same bytes.

## Decision

- **A token per session.** Before starting a shell, Woop generates a 128-bit random token, given as 32 hexadecimal characters (`shell_integration::MarkToken`). It is made with the standard library's randomly keyed SipHash (`RandomState`, seeded from the operating system), so no dependency is added.
- **Passed in the environment.** The token reaches the shell in the `WOOP_TOKEN` environment variable. The script reads it into a shell variable that is not exported (keeping only hexadecimal digits, since bash and zsh write it into the prompt string) and removes it from the environment, so programs started from the shell do not inherit it. bash and zsh do this before loading the user's configuration; PowerShell and fish load the user's profile first, so programs started by the profile itself still see it. It is never written to the integration files or put on the command line; both can be read by others: the files are shared by all sessions, and command lines are visible to other users on Unix.
- **Carried by every mark.** The scripts add a `woop=<token>` option to each mark (`133;A;woop=…`, `133;D;0;woop=…`).
- **Required by the terminal.** `Terminal::require_mark_token` makes the mark scanner ignore OSC 133 marks without the expected token. Woop's application always requires it, also for shells without integration, which then simply produce no blocks. The token must be set before any output is pumped. History clears and over-long sequences are detected as before, since they are not marks.

## Alternatives considered

- **Token on the command line or in the integration files.** Simpler to pass, but other local users can read command lines on Unix, and the files are shared between sessions.
- **A random number dependency.** Would give a cryptographic generator directly, but adds a crate for something the standard library already provides adequately: an attacker sees no output of the keyed hash before guessing.
- **Accepting unmarked sequences when no token arrives.** Would keep third-party integrations working, but leaves the forgery open for every shell Woop does not integrate.

## Consequences

- Displayed files and remote hosts can no longer create blocks, change exit codes, or turn the command editor on or off.
- A program started from the shell can still read the token if it inspects the shell's memory or its prompt variable; such a program already runs as the user and is outside this threat.
- Output that replays this session's own marks, such as a recording of the session made with `script` and shown later in the same session, still carries a valid token. This needs the user to record and replay their own session.
- OSC 133 marks from other tools (a prompt framework that emits its own) are ignored; only Woop's scripts produce blocks. The scripts recognise their own marks by the token, so they still wrap a prompt that carries such marks.
- In bash and zsh the token is part of the prompt string. A user configuration that exports `PS1` passes it to every program; with sudo set to keep `PS1`, a program running as another user could then read it.
- The token's randomness relies on the standard library seeding `RandomState` from the operating system, which it does today but does not promise. If that changes, Woop should take the bits from the operating system directly.
- A program writing to the terminal at the exact moment the shell prints a mark could, in theory, insert bytes inside it and change its letter or exit code; this needs a precise race and only yields a wrong mark.
