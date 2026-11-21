//! Terminal emulation core.
//!
//! Parses VT/ANSI escape sequences, maintains the screen grid and scrollback,
//! and connects them to a shell running in a pseudo-terminal.
//!
//! - [`Terminal`] is the pure emulation state: feed it bytes, read the screen.
//! - [`Screen`] is a snapshot of the visible cells, ready to be drawn.
//! - [`Session`] runs a shell in a pseudo-terminal and keeps a [`Terminal`]
//!   up to date with its output.
//! - [`input`] translates key presses and pasted text into terminal input.

pub mod input;
mod screen;
mod session;
mod terminal;

pub use screen::{Cell, CellStyle, Color, Cursor, CursorShape, Screen};
pub use session::Session;
pub use terminal::{CursorPosition, Terminal, TerminalEvent, TerminalMode, TerminalSize};
pub use woop_pty::{ExitStatus, PtyError, ShellCommand};
