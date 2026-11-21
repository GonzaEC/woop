# 0001. Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Woop is a public project that expects outside contributors. The reasons behind early choices (language, UI framework, crate layout) are easy to lose once the code exists, and without them later contributors either repeat old debates or undo decisions without knowing their cost.

## Decision

We will record every architecturally significant decision as an Architecture Decision Record in `docs/adr/`, using the format in [template.md](template.md). Records are numbered sequentially, reviewed in pull requests like code, and never rewritten after acceptance: a changed decision gets a new record that supersedes the old one.

## Alternatives considered

- **Design notes in issues or discussions.** Hard to find later and not versioned with the code.
- **A single design document.** Grows without structure and loses the history of how decisions changed.

## Consequences

- Contributors can understand why the code is shaped the way it is.
- Significant changes require a short written justification, which slows them slightly but makes them reviewable.
- The index in [README.md](README.md) must be kept up to date.
