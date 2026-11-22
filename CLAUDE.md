@AGENTS.md

## Claude Code

Project skills live in `.claude/skills/` and agents in `.claude/agents/`:

- `/verify`: run the same checks as CI and review the diff against the project rules.
- `/new-adr`: create a new Architecture Decision Record.
- `/ship`: branch, verify, review, commit and open a pull request.
- `/explain`: walk through a change or a piece of code, explaining the Rust and terminal concepts involved.
- `/learn`: turn a lesson (bug, CI failure, correction) into an update of the guide, skills, agents, tests or ADRs.
- `/fix-bug`: reproduce a bug with a failing test, fix the root cause and record the lesson.
- `/add-dependency`: evaluate a crate (need, maintenance, security, licence, cost) before adding it.
- `reviewer` agent: general review of a change before it is shipped.
- `security-reviewer` agent: attacker-minded review of anything that handles untrusted output, input, files, processes or dependencies. Security comes first.
- `performance-reviewer` agent: measures hot-path changes in release builds and reports numbers.

Run the specialist agents according to the "Reviews" table in `AGENTS.md`; they work independently, so launch the ones that apply in parallel.
