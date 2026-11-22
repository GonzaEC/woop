---
name: learn
description: Turn a lesson into a lasting improvement to Woop's guidance - AGENTS.md, skills, agents, tests or ADRs - so the same mistake is not repeated. Use after a bug, a failing or flaky CI run, a correction from the user, a review finding, a surprising platform behaviour, or at the end of a pull request; and when the user asks to update the agents or skills with what was learned.
---

# Learn from what happened

The project's guidance must improve with every mistake. A lesson that only lives in a conversation is lost.

## 1. Collect the lessons

Look back over the work: failed commands, CI failures, review findings, user corrections, workarounds, things that took several attempts, assumptions that turned out wrong. For each one, write one sentence: *what would have prevented it?*

Skip lessons that are one-off accidents with no general rule behind them.

## 2. Put each lesson where it will be applied

| The lesson is... | Put it in |
| --- | --- |
| A bug in the code | A regression test that fails without the fix (always), plus the fix |
| A rule every contributor should follow, with any tool | `AGENTS.md` (and `CONTRIBUTING.md` if it concerns humans) |
| A step to run when verifying or shipping | `.claude/skills/verify` or `.claude/skills/ship` |
| Something a review should catch | The checklist of `reviewer`, `security-reviewer` or `performance-reviewer` in `.claude/agents/` |
| A decision with alternatives and trade-offs | A new ADR (use the `new-adr` skill) |
| Pending work | `docs/roadmap.md` |

Prefer making an existing rule more precise over adding a new one, and remove rules that turned out wrong. Keep the files short: a long guide is not read.

## 3. Rules for what goes in the repository

The repository is public:

- English, professional, about Woop on its own terms; never name or compare other terminal applications.
- Nothing personal: no names, machines, local paths, accounts or private preferences. Those belong in the assistant's private memory, if anywhere.
- State the rule and, briefly, the reason; the reason is what lets someone apply it to a new case.

## 4. Ship it

Include the updates in the pull request that taught the lesson when possible, otherwise in a separate `docs:` pull request. Tell the user, in a short table, each lesson and where it now lives.
