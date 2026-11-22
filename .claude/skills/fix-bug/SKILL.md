---
name: fix-bug
description: Fix a bug in Woop the disciplined way - reproduce it with a test that fails, find the root cause, fix it, prove the test passes, check for the same bug elsewhere, and record the lesson. Use when the user reports something broken, a test or CI job fails, or a review finds a defect.
---

# Fix a bug

A bug is fixed when a test proves it cannot come back ([ADR 0012](../../../docs/adr/0012-security-tests-performance-priorities.md)).

## 1. Understand and assess

- Restate the bug: what happens, what should happen, on which platforms and shells.
- **Security first:** can program output, pasted text or a file trigger it? A crash, hang or memory growth triggered by output is a denial of service; anything that types into the shell, touches the clipboard or files is worse. If so, treat it as a security issue: run the `security-reviewer` agent on the area, and do not describe the exploit in public issue text before the fix is merged.

## 2. Reproduce with a failing test

- Write the smallest test that shows the bug, at the lowest layer where it occurs: a unit test in `woop-term` for parsing, blocks or input encoding; an end-to-end test in `tests/` when a real process is needed.
- Run it and **see it fail** for the reason you expect. A test that has never failed proves nothing.
- If it cannot be reproduced automatically (rendering, a window event), say so, reproduce it by hand with the user (see "Manual testing" in `AGENTS.md`), and add temporary logging rather than guessing. Remove the logging afterwards.

## 3. Find the root cause

Fix the cause, not the symptom. Ask why until the answer is in Woop's code or a documented behaviour of a dependency or platform. Do not weaken a test, widen a timeout or add a retry to make a failure disappear: a flaky test is a bug in the test or the code.

## 4. Fix and prove

- Make the smallest change that fixes the cause.
- The new test passes, and so does everything else: run the `verify` skill.
- Look for the same mistake elsewhere (`grep` for the pattern) and fix those too, with tests.

## 5. Learn and ship

- Run the `learn` skill: what rule or check would have prevented this bug?
- Ship with the `ship` skill. The pull request's Testing section names the regression test and states that it fails without the fix.
