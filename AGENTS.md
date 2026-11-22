# Agent guide

Instructions for AI coding agents (and a quick reference for humans) working on Woop. Read this before making changes. Human contributors should also read [CONTRIBUTING.md](CONTRIBUTING.md).

## Project

Woop is a fast, lightweight, GPU-accelerated terminal written in Rust, targeting Windows, macOS and Linux. Core principles: security, reliability, performance, low memory use, no accounts, no telemetry ([ADR 0008](docs/adr/0008-no-accounts-no-telemetry.md)).

- Architecture overview: [docs/architecture.md](docs/architecture.md)
- Decisions and their reasons: [docs/adr/](docs/adr/README.md)
- Milestones: [docs/roadmap.md](docs/roadmap.md)

## Priorities

Judge every change in this order; when two conflict, the earlier one wins, and all three come before new features ([ADR 0012](docs/adr/0012-security-tests-performance-priorities.md)).

1. **Security is paramount.** A terminal runs the user's shell and displays output from untrusted sources (files, remote machines).
   - Treat program output as untrusted input. Anything that lets output act beyond drawing text (clipboard writes, replies containing data, links, files) is off by default or needs an explicit user action, and is bounded in size.
   - Never type anything into the shell that the user did not, except the standard replies needed to run (cursor position, device attributes). Pasted text must not carry control sequences.
   - Bound every buffer, channel and history fed by program output.
   - No `unsafe` without an ADR. Write files only to the user's own directories, with permissions that keep other users out. No network access except user-configured AI providers. Never log or store secrets.
   - Every new dependency needs a reason and an evaluation (maintenance, vulnerabilities, licence, `unsafe`, cost); see the [`add-dependency`](.claude/skills/add-dependency/SKILL.md) skill. Prefer the standard library and well-maintained crates.
   - `deny.toml`, `.github/workflows/` and `.github/dependabot.yml` are security controls: changing them is a security change. When a new advisory makes the dependency check fail, upgrade the crate; if that is impossible (the fix must come from upstream), add the advisory to `ignore` in `deny.toml` with how the crate reaches Woop and why the risk is acceptable, after a security review. Never weaken the policy to make CI pass.
2. **Tests, so nothing breaks.** Every behaviour change has tests; every bug fix starts with a test that fails without it (see the [`fix-bug`](.claude/skills/fix-bug/SKILL.md) skill). Code that parses untrusted input must never panic or hang on arbitrary bytes, and has tests that prove it.
3. **Performance.** Keep the read, parse and draw paths free of per-byte or per-cell allocations, locks and blocking calls, and do no work while the shell is idle. Changes to hot paths include measurements from release builds.

## Commands

```sh
cargo build                                            # build everything
cargo test --workspace                                 # all tests, including end-to-end PTY tests
cargo fmt --all                                        # format
cargo clippy --workspace --all-targets -- -D warnings  # lint; CI treats warnings as errors
cargo deny check                                       # dependency advisories, licences, sources (cargo install cargo-deny)
cargo run -p woop-term --example headless -- "echo hi" # run the core without a window
```

Run `fmt`, `clippy` and `test` before declaring any change finished, and `cargo deny check` when dependencies change. These are exactly what CI checks.

On Windows, if compiling a dependency crashes `rustc` with `STATUS_STACK_BUFFER_OVERRUN`, raise its stack size for that build: `RUST_MIN_STACK=67108864 cargo build` (PowerShell: `$env:RUST_MIN_STACK = 67108864`).

### Manual testing

Automated tests cannot judge how the window looks or feels (rendering, keyboard, mouse, clipboard). Run the app with `cargo run -p woop-app` and ask a person to try the change, listing what to check. Agents must not inject synthetic keyboard or mouse input into, or take screenshots of, a person's desktop without asking first: other windows may be in front and receive the input.

## Layout and boundaries

| Crate | Responsibility | May depend on |
| --- | --- | --- |
| `woop-pty` | Pseudo-terminals and child processes | nothing in the workspace |
| `woop-term` | Terminal emulation (`Terminal`) and sessions (`Session`) | `woop-pty` |
| `woop-config` | Configuration, themes, keybindings | nothing in the workspace |
| `woop-ai` | Optional assistant providers | `woop-config` |
| `woop-app` | Window, rendering, interface (iced) | all of the above |

- Never let a lower crate depend on a higher one ([ADR 0004](docs/adr/0004-layered-cargo-workspace.md)).
- Third-party types stay behind Woop's own wrapper types; do not expose library types in public APIs of `woop-pty` or `woop-term`.
- `Terminal` performs no I/O; process handling belongs in `Session` and `woop-pty` ([ADR 0007](docs/adr/0007-dedicated-pty-reader-thread.md)).

## Code conventions

- Stable Rust, edition 2024. No `unsafe` in Woop's own code without an ADR.
- Every public item has a doc comment. Errors use `thiserror` enums per crate; avoid `unwrap()` outside tests.
- Platform-specific code goes behind `cfg` attributes and must keep all three platforms compiling.
- Keep allocations and blocking calls off the render path.
- Add tests with every behaviour change: unit tests next to the code, end-to-end tests in `tests/`. Prefer real processes over mocks for PTY behaviour.
- Tests with real shells must not depend on timing. A shell may echo input before it prints its prompt, so match on content (for example, a line *ending* in the expected output) rather than on exact lines or order.
- Tests run on all three platforms. Do not parse another platform's paths with `std::path`: on Unix, `Path` does not treat `\` as a separator.

## Platform conventions

- **Keyboard shortcuts** follow each platform (`keys::Shortcuts` in `woop-app`). Windows: Ctrl+C copies while text is selected and interrupts otherwise, Ctrl+V pastes. Linux: Ctrl+Shift+C and Ctrl+Shift+V, leaving Ctrl+C and Ctrl+V to the shell. macOS: Cmd+C and Cmd+V. A new shortcut must not take a key the shell needs on that platform.
- **Shell integration** scripts live in `crates/woop-term/shell/` and are embedded in the binary ([ADR 0011](docs/adr/0011-shell-integration-and-command-blocks.md)). They:
  - wrap the user's prompt and configuration, never replace them, and keep the last exit status visible to the user's prompt;
  - keep LF line endings (`.gitattributes` enforces it);
  - support the oldest shells Woop targets: Windows PowerShell 5.1 has no `` `e `` escape (use `[char]27`), and bash 3.2 (macOS) has no `PS0`, so commands are marked from the `DEBUG` trap, which does not fire for subshells such as `( ... )`.

  `crates/woop-term/tests/shell_integration.rs` runs the scripts in real shells. When developing on Windows, the bash and zsh scripts are only exercised by CI; check its logs to confirm those tests ran.

## Reviews

Every non-trivial change gets a general review before it is merged. Two areas also get a specialist review, whose checklists in `.claude/agents/` are plain Markdown and can be followed by any person or tool:

| Review | When | Checklist |
| --- | --- | --- |
| General | Every non-trivial change | [`reviewer.md`](.claude/agents/reviewer.md) |
| Security | The change touches program output handling (escape sequences, marks, replies), input or paste, the clipboard, process spawning, files Woop writes, shell integration scripts, dependencies, CI workflows, `deny.toml` or secrets | [`security-reviewer.md`](.claude/agents/security-reviewer.md) |
| Performance | The change touches reading, parsing, block tracking, snapshots or drawing, start-up or memory use | [`performance-reviewer.md`](.claude/agents/performance-reviewer.md) |

Security findings are fixed before anything else in the pull request.

## Keeping this guide current

This guide, the skills and the review checklists must learn from experience. When a bug, a CI failure, a review finding or a correction reveals something that a rule would have prevented, update the right place in the same pull request: a regression test for the bug, `AGENTS.md` for rules, a skill for workflow steps, a review checklist for what reviews should catch, an ADR for decisions. The [`learn`](.claude/skills/learn/SKILL.md) skill describes the process. Keep entries short, precise and free of personal details.

## Decisions

Architecturally significant changes (new structural dependency, crate boundary change, cross-cutting rule) need a new ADR in `docs/adr/`, using `docs/adr/template.md`, added to the index in the same pull request. Never edit an accepted ADR; supersede it with a new one.

## Writing style

The repository is public. Documentation, comments, commit messages and pull request descriptions must be:

- In English, clear and professional.
- About Woop on its own terms. Describe features by what they do; **never compare Woop with, or mention, other terminal applications or commercial products**. Naming a library that is an actual dependency (in `Cargo.toml` or in an ADR) is fine.

## Commits and pull requests

- Work on a branch; never push to `main`. Changes merge through squash-merged pull requests that pass CI ([ADR 0009](docs/adr/0009-pull-requests-gated-by-ci.md)). GitHub enforces this for everyone, administrators included: `main` accepts no direct or force pushes, a pull request needs the four CI checks green, an up-to-date branch (`gh pr update-branch <number>` when it falls behind) and resolved conversations, and only squash merges are allowed. Branches are deleted when merged.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org): `feat(term): ...`, `fix(pty): ...`, `docs: ...`.
- Do not add tool-attribution trailers or footers (such as `Co-Authored-By` lines for AI tools, or "generated with" notes) to commits or pull requests. The person submitting a change is its author and is responsible for understanding and testing it.
- Pull request descriptions use the template: a summary of what and why, and how it was tested on which platforms.
- Commit and push only after the checks pass. Do not chain them after a check in a single shell command, where a failure would not stop the push.
- Read a file before overwriting it, even configuration that seems new (`.gitattributes`, workflows).
