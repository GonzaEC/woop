//! Pseudo-terminal (PTY) management.
//!
//! Spawns shell processes attached to a pseudo-terminal and exposes their
//! input and output streams. Uses ConPTY on Windows and the native PTY
//! interface on Unix-like systems.
