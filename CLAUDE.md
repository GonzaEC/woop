@AGENTS.md

## Claude Code

Project skills live in `.claude/skills/` and agents in `.claude/agents/`:

- `/verify`: run the same checks as CI and review the diff against the project rules.
- `/new-adr`: create a new Architecture Decision Record.
- `/ship`: branch, verify, commit and open a pull request.
- `/explain`: walk through a change or a piece of code, explaining the Rust and terminal concepts involved.
- `reviewer` agent: independent review of a change before it is shipped.
