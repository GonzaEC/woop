---
name: verify
description: Run the same checks as Woop's CI (fmt, clippy with warnings as errors, tests) and review the current diff against the project rules in AGENTS.md. Use before declaring a change finished, before committing, or when asked to check, validate or verify the code.
---

# Verify a change

Run every step, in order, from the repository root. Do not stop at the first failure: collect all problems, then fix or report them.

## 1. Automated checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- If formatting fails, run `cargo fmt --all` and re-check.
- If clippy fails, fix the code. Do not silence a lint with `#[allow]` unless there is a clear reason, stated in a comment next to it.
- If a test fails, find the cause. Never weaken or delete a test to make it pass.

## 2. Review the diff against the project rules

Inspect `git diff main...HEAD` plus uncommitted changes and check:

- **Crate boundaries** follow the table in `AGENTS.md`; no library types leak into public APIs of `woop-pty` or `woop-term`.
- **Cross-platform:** platform-specific code is behind `cfg` and both branches compile in principle; paths and line endings are not hard-coded for one OS.
- **Tests** cover the new behaviour.
- **Docs:** public items have doc comments; `docs/architecture.md`, `docs/roadmap.md` or the README are updated if behaviour or structure changed.
- **ADR needed?** A new structural dependency, a crate boundary change or a cross-cutting rule requires a record (use the `new-adr` skill).
- **Writing style:** all added text is in English and professional, and nothing compares Woop with, or names, other terminal applications or commercial products.
- **No attribution trailers** in commit messages (`git log main..HEAD`).

## 3. Report

Summarise in a short list: each check, passed or failed, and for failures the file, line and fix. State plainly what could not be verified (for example, behaviour on an operating system other than the current one, which only CI covers).
