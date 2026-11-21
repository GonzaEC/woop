---
name: new-adr
description: Create a new Architecture Decision Record in docs/adr/ for Woop, numbered and indexed. Use when a change adds a structural dependency, moves a crate boundary, sets a cross-cutting rule, supersedes an earlier decision, or when the user asks to record or document a decision.
---

# Create an Architecture Decision Record

## 1. Gather the decision

Before writing, make sure you know:

- the problem and the forces involved (context)
- what was decided
- at least two alternatives and why each was rejected
- the consequences, including the negative ones

If any of these is unclear, ask the user. An ADR records a decision someone made; do not invent reasons or alternatives that were not actually considered.

## 2. Write the record

1. List `docs/adr/` and take the next number after the highest `NNNN-*.md` (four digits, zero-padded).
2. Copy the structure of `docs/adr/template.md` into `docs/adr/NNNN-short-kebab-title.md`.
3. Title in imperative form ("Use X for Y", "Read output on a dedicated thread").
4. Status `Accepted` if the decision is already made, otherwise `Proposed`. Date is today.
5. Keep it short: a reader should understand it in two minutes.

## 3. Supersede, never edit

If the new decision replaces an earlier one, do not rewrite the old record's content. Only change its status line to `Superseded by [NNNN](NNNN-title.md)`, and mention the old record in the new one's context.

## 4. Update the index and related docs

- Add a row to the index table in `docs/adr/README.md` (and update the status of any superseded record).
- If the decision changes what `docs/architecture.md` or `AGENTS.md` describe, update them in the same change.

## Style

English, professional, about Woop on its own terms. Never compare Woop with, or name, other terminal applications or commercial products; naming a library that is an actual dependency is fine.
