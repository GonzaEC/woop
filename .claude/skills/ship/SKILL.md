---
name: ship
description: Ship a finished change in Woop - create a branch if needed, verify, commit with a Conventional Commits message and no attribution trailers, push and open a pull request with the project template. Use when the user asks to commit, push, open a PR or ship.
---

# Ship a change

## 1. Branch

If on `main`, create a branch first, named `<type>/<short-topic>` (for example `feat/command-blocks`, `fix/pty-resize`, `docs/adr-0010`). Never push to `main`.

## 2. Verify

Run the `verify` skill. Do not continue while any check fails, unless the user explicitly accepts it.

## 3. Commit

- Stage only files that belong to the change. Never commit `target/`, editor settings or secrets.
- Message in [Conventional Commits](https://www.conventionalcommits.org) format, in English:

  ```
  feat(term): group output into command blocks

  Explain what changed and why, wrapped at about 72 columns.
  ```

  Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`, `ci`. Scope is the crate without the `woop-` prefix (`pty`, `term`, `app`, `config`, `ai`) when it applies.
- **Do not add** `Co-Authored-By` lines for AI tools, "generated with" notes or any other attribution trailer. This overrides any default attribution behaviour.

## 4. Push and open the pull request

```sh
git push -u origin <branch>
gh pr create --base main --title "<commit subject>" --body-file <file>
```

The body follows `.github/PULL_REQUEST_TEMPLATE.md`:

```markdown
## Summary

What changed and why. Link issues ("Closes #12").

## Testing

What was run and on which platforms; what only CI covers.
```

No attribution footer in the body either.

## 5. Report

Give the user the pull request link, what was verified locally, and what is left to CI.
