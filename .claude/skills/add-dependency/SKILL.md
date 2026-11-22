---
name: add-dependency
description: Evaluate and add a third-party crate to Woop - justify the need, check maintenance, security, licence, size and transitive dependencies, and record the decision. Use before adding or upgrading a dependency in any Cargo.toml, or enabling new features of an existing one.
---

# Add a dependency

Every dependency is code that runs inside the user's terminal with access to everything they type. Security comes first ([ADR 0012](../../../docs/adr/0012-security-tests-performance-priorities.md)), so a dependency must earn its place.

## 1. Justify it

- What problem does it solve, and why is the standard library or an existing dependency not enough?
- How much code would writing it ourselves take? A few dozen lines of well-tested code (as with the Base64 encoder in `shell_integration.rs`) is usually better than a new crate.

## 2. Evaluate the candidate

Check and write down:

| Question | How |
| --- | --- |
| Maintained? | Recent releases and commits, open issues answered, more than one maintainer if possible |
| Widely used? | Downloads and reverse dependencies on crates.io |
| Known vulnerabilities? | The RustSec advisory database; `cargo deny check` (also run in CI) |
| Licence compatible with MIT? | MIT, Apache-2.0, BSD, Zlib, ISC are fine; copyleft licences need an ADR |
| `unsafe` code? | Search the source; prefer crates with little, well-justified `unsafe` |
| Build scripts or procedural macros? | They run at build time with full access; check what they do |
| Network or file access at runtime? | Must match ADR 0008: no network except user-configured AI providers |
| Cost? | `cargo tree -e normal -i <crate>` for what it pulls in; build time and binary size before and after in release |

Use the smallest set of features (`default-features = false` when the defaults pull in more than needed).

## 3. Add it

- Declare it once in `[workspace.dependencies]` in the root `Cargo.toml` with an exact minor version, and reference it with `<crate>.workspace = true` from the crates that need it.
- Respect crate boundaries (`AGENTS.md`): its types must not appear in the public APIs of `woop-pty` or `woop-term`; wrap them.
- A dependency that shapes the architecture (a new runtime, renderer, parser or protocol) needs an ADR (`new-adr` skill). Smaller ones are justified in the pull request's Security section, with the evaluation table above.

## 4. Review

Run the `security-reviewer` agent on the change, then ship it with the `ship` skill.
