# 0002. Use Rust as the implementation language

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Woop's core promise is a terminal that is fast, light on memory and responsive under heavy output. A terminal runs all day, parses untrusted byte streams from arbitrary programs and must work on Windows, macOS and Linux. The language must give predictable performance without a garbage collector, memory safety when parsing untrusted input, and good cross-platform support for system APIs such as pseudo-terminals and GPU rendering.

## Decision

We will write Woop in Rust, on the stable toolchain, with the edition and minimum supported version declared in the workspace `Cargo.toml`.

## Alternatives considered

- **C or C++.** Comparable performance, but memory safety would depend on discipline alone, which is a poor fit for a parser of untrusted input maintained by a community.
- **Go.** Simple and productive, but its garbage collector and weaker GPU and windowing ecosystem conflict with low-latency rendering.
- **TypeScript on an embedded web engine.** Fast to build interfaces, but the runtime footprint contradicts the goal of a lightweight application.

## Consequences

- Memory safety and data-race freedom are checked at compile time.
- A mature ecosystem exists for every layer: terminal emulation, pseudo-terminals, GPU rendering and serialization.
- Rust has a steep learning curve, which raises the barrier for some contributors. Clear crate boundaries and documentation help offset this.
- Compile times are longer than in dynamic languages; CI uses caching to compensate.
