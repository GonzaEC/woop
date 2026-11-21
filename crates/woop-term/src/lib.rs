//! Terminal emulation core.
//!
//! Parses VT/ANSI escape sequences, maintains the screen grid and scrollback,
//! and groups shell activity into command blocks using shell integration
//! markers (OSC 133).
