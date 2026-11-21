---
name: explain
description: Walk through a change, pull request, file or concept in Woop and explain it for someone learning Rust and terminal internals - ownership, traits, threads, channels, PTYs, escape sequences. Use when the user asks how or why something works, wants to understand a PR before merging, or is preparing to explain the code to someone else.
---

# Explain code for learning

The goal is understanding, not a summary. The reader should be able to explain the code themselves afterwards, for example in a technical interview.

## How to explain

1. **Answer in the user's language** (the language of their message), even though the code and docs are in English.
2. **Start with the big picture:** what problem this code solves and where it sits in the architecture (`docs/architecture.md`). Link the relevant ADR if one exists.
3. **Walk through the code in execution order**, not file order. Reference locations as `path:line`.
4. **Explain each Rust concept the first time it appears**, briefly and with this code as the example: ownership and borrowing, `&` vs `&mut`, `Result` and `?`, traits and `impl Trait`, `Box<dyn Trait>`, `Send`/`Sync`, threads and channels, `Drop`, `#[cfg]`, lifetimes when visible.
5. **Explain the domain concepts** too: what a pseudo-terminal is, why ConPTY queries the cursor position, what escape sequences do, why output is read on another thread.
6. **Point out the design decisions and their trade-offs:** why this approach and not the obvious alternative.
7. **End with three to five questions** an interviewer could ask about this code, each with a short model answer.

## Scope

- For a pull request or branch: start from `git diff main...HEAD`.
- For a file or function: read it fully, plus whatever it calls that is needed to understand it.
- Do not modify code while explaining unless the user asks.
