//! Pseudo-terminal (PTY) management.
//!
//! Spawns shell processes attached to a pseudo-terminal and exposes their
//! input and output streams. Uses ConPTY on Windows and the native PTY
//! interface on Unix-like systems.
//!
//! ```no_run
//! use std::io::Read;
//! use woop_pty::{Pty, PtySize, ShellCommand};
//!
//! let mut pty = Pty::spawn(&ShellCommand::default_shell(), PtySize::new(24, 80))?;
//! let mut reader = pty.take_reader()?;
//! pty.write_all(b"echo hello\r")?;
//!
//! let mut buf = [0u8; 4096];
//! let n = reader.read(&mut buf)?;
//! println!("{}", String::from_utf8_lossy(&buf[..n]));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};

/// Errors produced while creating or controlling a pseudo-terminal.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// The pseudo-terminal could not be created.
    #[error("failed to open pseudo-terminal: {0}")]
    Open(String),
    /// The shell process could not be started.
    #[error("failed to spawn `{program}`: {message}")]
    Spawn {
        /// Program that failed to start.
        program: String,
        /// Underlying error message.
        message: String,
    },
    /// The pseudo-terminal could not be resized.
    #[error("failed to resize pseudo-terminal: {0}")]
    Resize(String),
    /// An I/O stream of the pseudo-terminal could not be obtained.
    #[error("pseudo-terminal stream unavailable: {0}")]
    Stream(String),
    /// Reading from or writing to the pseudo-terminal failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Result type used throughout this crate.
pub type Result<T, E = PtyError> = std::result::Result<T, E>;

/// Size of the terminal, in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    /// Number of visible rows.
    pub rows: u16,
    /// Number of visible columns.
    pub cols: u16,
}

impl PtySize {
    /// Creates a size of `rows` by `cols` cells.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self { rows, cols }
    }
}

impl From<PtySize> for portable_pty::PtySize {
    fn from(size: PtySize) -> Self {
        Self {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// Description of the process to run inside the pseudo-terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand {
    /// Program to execute.
    pub program: OsString,
    /// Arguments passed to the program.
    pub args: Vec<OsString>,
    /// Working directory. Defaults to the current directory when `None`.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables, added to the inherited environment.
    pub env: Vec<(OsString, OsString)>,
}

impl ShellCommand {
    /// Creates a command that runs `program` with no arguments.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    /// Appends an argument.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Sets the working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Adds an environment variable.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Returns the user's default shell.
    ///
    /// On Windows this is PowerShell 7 (`pwsh.exe`) when it is installed,
    /// falling back to Windows PowerShell. On Unix it is `$SHELL`, falling
    /// back to `/bin/sh`.
    pub fn default_shell() -> Self {
        #[cfg(windows)]
        {
            let program = if find_in_path("pwsh.exe").is_some() {
                "pwsh.exe"
            } else {
                "powershell.exe"
            };
            Self::new(program).arg("-NoLogo")
        }
        #[cfg(not(windows))]
        {
            let program = std::env::var_os("SHELL")
                .filter(|shell| !shell.is_empty())
                .unwrap_or_else(|| "/bin/sh".into());
            Self::new(program)
        }
    }

    fn to_builder(&self) -> CommandBuilder {
        let mut builder = CommandBuilder::new(&self.program);
        builder.args(&self.args);
        match &self.cwd {
            Some(cwd) => builder.cwd(cwd),
            None => {
                if let Ok(cwd) = std::env::current_dir() {
                    builder.cwd(cwd);
                }
            }
        }
        builder.env("TERM", "xterm-256color");
        builder.env("COLORTERM", "truecolor");
        for (key, value) in &self.env {
            builder.env(key, value);
        }
        builder
    }
}

#[cfg(windows)]
fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Exit status of a process that ran inside a pseudo-terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    /// Exit code reported by the process.
    pub code: u32,
}

impl ExitStatus {
    /// Returns `true` when the process exited with code zero.
    pub fn success(self) -> bool {
        self.code == 0
    }
}

/// A process running inside a pseudo-terminal.
///
/// Dropping a `Pty` closes the pseudo-terminal and terminates the process.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: PtySize,
}

impl Pty {
    /// Opens a pseudo-terminal of the given size and starts `command` in it.
    pub fn spawn(command: &ShellCommand, size: PtySize) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(size.into())
            .map_err(|err| PtyError::Open(err.to_string()))?;

        let child = pair
            .slave
            .spawn_command(command.to_builder())
            .map_err(|err| PtyError::Spawn {
                program: command.program.to_string_lossy().into_owned(),
                message: err.to_string(),
            })?;
        // The child holds its own handle to the slave side. Releasing ours
        // lets the reader observe end-of-file once the child exits.
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|err| PtyError::Stream(err.to_string()))?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            size,
        })
    }

    /// Returns a reader for the process output.
    ///
    /// Reads block until output is available, so the reader is normally
    /// moved to a dedicated thread. A read of zero bytes, or an error, means
    /// the pseudo-terminal was closed.
    pub fn take_reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master
            .try_clone_reader()
            .map_err(|err| PtyError::Stream(err.to_string()))
    }

    /// Sends input to the process, as if typed on a keyboard.
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Changes the size of the pseudo-terminal.
    pub fn resize(&mut self, size: PtySize) -> Result<()> {
        if size != self.size {
            self.master
                .resize(size.into())
                .map_err(|err| PtyError::Resize(err.to_string()))?;
            self.size = size;
        }
        Ok(())
    }

    /// Returns the current size of the pseudo-terminal.
    pub fn size(&self) -> PtySize {
        self.size
    }

    /// Returns the exit status if the process has finished, without blocking.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self.child.try_wait()?.map(|status| ExitStatus {
            code: status.exit_code(),
        }))
    }

    /// Terminates the process.
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill()?;
        Ok(())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
    }
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pty")
            .field("size", &self.size)
            .field("pid", &self.child.process_id())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_collects_arguments_and_environment() {
        let command = ShellCommand::new("sh")
            .arg("-c")
            .arg("true")
            .cwd("/tmp")
            .env("KEY", "value");

        assert_eq!(command.program, "sh");
        assert_eq!(command.args, ["-c", "true"]);
        assert_eq!(command.cwd, Some(PathBuf::from("/tmp")));
        assert_eq!(command.env, [("KEY".into(), "value".into())]);
    }

    #[test]
    fn default_shell_is_not_empty() {
        assert!(!ShellCommand::default_shell().program.is_empty());
    }

    #[test]
    fn exit_status_success() {
        assert!(ExitStatus { code: 0 }.success());
        assert!(!ExitStatus { code: 1 }.success());
    }
}
