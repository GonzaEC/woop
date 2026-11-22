---
name: ship
description: Ship a finished change in Woop - create a branch if needed, verify, commit with a Conventional Commits message and no attribution trailers, push and open a pull request with the project template. Use when the user asks to commit, push, open a PR or ship.
---

# Ship a change

## 1. Branch

If on `main`, create a branch first, named `<type>/<short-topic>` (for example `feat/command-blocks`, `fix/pty-resize`, `docs/adr-0010`). Never push to `main`.

## 2. Verify

Run the `verify` skill. Do not continue while any check fails, unless the user explicitly accepts it.

Run the commit and push as separate steps after the checks have passed. Never chain them after a check in one shell command (`cargo test; git commit ...; git push`): a failing check would not stop the push, and a broken commit would reach the pull request.

## 3. Review

For a non-trivial change, run the reviews that apply according to the "Reviews" table in `AGENTS.md`: the `reviewer` agent, plus `security-reviewer` and `performance-reviewer` when the change touches their areas. They are independent; run them in parallel. Fix security findings first, then the rest, and verify again.

## 4. Commit

- Stage only files that belong to the change. Never commit `target/`, editor settings or secrets.
- Message in [Conventional Commits](https://www.conventionalcommits.org) format, in English:

  ```
  feat(term): group output into command blocks

  Explain what changed and why, wrapped at about 72 columns.
  ```

  Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`, `ci`. Scope is the crate without the `woop-` prefix (`pty`, `term`, `app`, `config`, `ai`) when it applies.
- **Do not add** `Co-Authored-By` lines for AI tools, "generated with" notes or any other attribution trailer. This overrides any default attribution behaviour.

## 5. Push and open the pull request

```sh
git push -u origin <branch>
gh pr create --base main --title "<commit subject>" --body-file <file>
```

The body follows `.github/PULL_REQUEST_TEMPLATE.md`:

```markdown
## Summary

What changed and why. Link issues ("Closes #12").

## Security

What untrusted input (program output, paste, files) reaches the change, and how it is bounded; or "No impact" with the reason.

## Testing

What was run and on which platforms; what only CI covers; the regression test for a bug fix.

## Performance

Release-build measurements for hot-path changes, or "Not on a hot path".
```

No attribution footer in the body either.

## 6. Follow CI

Wait for the checks (`gh pr checks <number> --watch`). If one fails, read the failing log (`gh run view <run-id> --log-failed`), fix the cause on the same branch and push again. When a test only runs on another platform (`#[cfg(unix)]`, a shell that is not installed locally), confirm in that job's log that it actually ran instead of being skipped.

## 7. Learn

If anything went wrong or surprised you on the way (a failed check, a CI failure, a review finding, a correction from the user), run the `learn` skill before reporting.

## 8. Report

Give the user the pull request link, what was verified locally, what CI covered, and anything that still needs a person to try by hand (see "Manual testing" in `AGENTS.md`). Merge only when the user approves.
