# 0009. Pull requests gated by cross-platform CI

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Woop targets Windows, macOS and Linux, and most contributors develop on only one of them. Platform-specific breakage (pseudo-terminal behaviour, paths, line endings) is easy to introduce and hard to notice locally.

## Decision

All changes reach `main` through pull requests. Each pull request must pass CI on all three platforms before merging: formatting (`cargo fmt`), lints with warnings as errors (`cargo clippy`), tests and a build. Commit messages follow [Conventional Commits](https://www.conventionalcommits.org), and pull requests are squash-merged so that `main` keeps one descriptive commit per change.

## Alternatives considered

- **Direct pushes to `main`.** Faster for a single maintainer, but skips cross-platform verification and leaves no review record.
- **CI on a single platform.** Cheaper, but misses exactly the class of bugs a cross-platform terminal is most exposed to.

## Consequences

- `main` always builds and passes tests on every supported platform.
- Each change has a reviewable record explaining what changed and why.
- CI time and minutes grow with the matrix; caching keeps this manageable.
