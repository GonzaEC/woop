# 0003. Native GPU-rendered interface with iced

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

Beyond drawing a character grid, Woop needs a rich interface: command blocks, an editor-like input, tabs, splits, a command palette and completion popups. The interface must stay smooth with large outputs, start quickly, use little memory and look consistent on Windows, macOS and Linux.

## Decision

We will build the interface natively in Rust with [iced](https://iced.rs), which renders through wgpu (Direct3D 12, Metal or Vulkan depending on the platform) and provides layout, widgets, text shaping and input handling. The terminal grid will be drawn by a custom iced widget optimised for many cells; the rest of the interface will use standard iced widgets.

## Alternatives considered

- **Web interface in an embedded webview.** Fastest way to build complex interfaces and mature terminal widgets exist, but each window carries a browser engine, raising memory use and startup time against the project's goals.
- **Rendering directly on wgpu with a custom UI layer.** Maximum control and performance, but text layout, widgets, focus and accessibility would all have to be built from scratch, delaying a usable release by months.
- **gpui.** High performance and designed for editor-like applications, but sparsely documented and with a frequently changing API, which makes it harder for contributors to learn.

## Consequences

- One language and one process for the whole application, with a small binary and low memory use.
- iced's Elm-style architecture (state, messages, view) keeps UI logic predictable and testable.
- iced is still pre-1.0; upgrades may require code changes, and some advanced widgets will need to be written in-house.
- The custom grid widget is performance-critical code that Woop owns and must benchmark.
