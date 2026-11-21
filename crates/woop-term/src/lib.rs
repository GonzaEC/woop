//! Terminal emulation core.
//!
//! Parses VT/ANSI escape sequences, maintains the screen grid and scrollback,
//! and connects them to a shell running in a pseudo-terminal.
//!
//! - [`Terminal`] is the pure emulation state: feed it bytes, read the screen.
//! - [`Session`] runs a shell in a pseudo-terminal and keeps a [`Terminal`]
//!   up to date with its output.

mod session;
mod terminal;

pub use session::Session;
pub use terminal::{CursorPosition, Terminal, TerminalEvent, TerminalSize};
pub use woop_pty::{ExitStatus, PtyError, ShellCommand};
