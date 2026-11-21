# Architecture Decision Records

This directory records the significant technical decisions made in Woop: what was decided, why, and what it costs. Each record is short, immutable once accepted, and numbered in order.

A decision that changes an earlier one does not edit it. A new record is added, and the old one is marked **Superseded by** the new record.

## When to write one

Write an ADR when a change:

- adds a dependency that shapes the architecture (UI framework, parser, async runtime...)
- changes crate boundaries or how crates communicate
- sets a cross-cutting rule (threading, error handling, configuration format, privacy)
- would make a new contributor ask "why was it done this way?"

Start from [the template](template.md) and add the record to the index below in the same pull request.

## Index

| # | Title | Status |
| --- | --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-use-rust.md) | Use Rust as the implementation language | Accepted |
| [0003](0003-native-gpu-ui-with-iced.md) | Native GPU-rendered interface with iced | Accepted |
| [0004](0004-layered-cargo-workspace.md) | Layered Cargo workspace | Accepted |
| [0005](0005-terminal-emulation-library.md) | Reuse an existing terminal emulation library | Accepted |
| [0006](0006-portable-pty.md) | portable-pty for pseudo-terminals | Accepted |
| [0007](0007-dedicated-pty-reader-thread.md) | Read process output on a dedicated thread | Accepted |
| [0008](0008-no-accounts-no-telemetry.md) | No accounts, no telemetry, optional bring-your-own AI | Accepted |
| [0009](0009-pull-requests-gated-by-ci.md) | Pull requests gated by cross-platform CI | Accepted |
