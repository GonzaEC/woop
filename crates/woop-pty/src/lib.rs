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
    /// Input was refused because too much is already waiting for the
    /// process, which is not reading it.
    #[error("the process is not reading its input; {0} bytes were not sent")]
    InputBacklog(usize),
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

    /// The command for the pseudo-terminal, with the program resolved to an
    /// absolute path ([`resolve_program`]).
    fn to_builder(&self) -> std::result::Result<CommandBuilder, String> {
        let mut builder = CommandBuilder::new(resolve_program(&self.program)?);
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
        Ok(builder)
    }
}

#[cfg(windows)]
fn find_in_path(program: &str) -> Option<PathBuf> {
    resolve_program_in(std::ffi::OsStr::new(program), std::env::var_os("PATH")).ok()
}

/// Resolves the program to run to an absolute path, so the operating system
/// never looks for it elsewhere. On Windows, a bare name that is not found
/// is otherwise looked up in the current directory of the parent process,
/// which may be a folder anyone could fill (a cloned repository, a download
/// folder) and would run a planted program.
fn resolve_program(program: &std::ffi::OsStr) -> std::result::Result<PathBuf, String> {
    resolve_program_in(program, std::env::var_os("PATH"))
}

/// [`resolve_program`] with the search path given:
/// - an absolute path is kept;
/// - another path (with a separator, such as `bin/sh` or `..\x.exe`) is
///   refused;
/// - a bare name is searched in the absolute directories of `search_path`
///   only (on Windows also with the extensions of `PATHEXT`), never in the
///   current directory.
fn resolve_program_in(
    program: &std::ffi::OsStr,
    search_path: Option<OsString>,
) -> std::result::Result<PathBuf, String> {
    let path = std::path::Path::new(program);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let text = program.to_string_lossy();
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    if text.is_empty() || text.contains(separators) || path.components().count() != 1 {
        return Err("must be an absolute path or a program name found in PATH".into());
    }
    let candidates = |dir: PathBuf| -> Vec<PathBuf> {
        std::iter::once(dir.join(program))
            .chain(with_extensions(&dir, &text, path))
            .collect()
    };
    search_path
        .iter()
        .flat_map(std::env::split_paths)
        // Empty and relative entries would mean the current directory.
        .filter(|dir| dir.is_absolute())
        .flat_map(candidates)
        .find(|candidate| is_program(candidate))
        .ok_or_else(|| format!("`{text}` was not found in PATH"))
}

/// On Windows, a name without an extension can be run with any extension
/// of `PATHEXT` (`pwsh` for `pwsh.exe`).
#[cfg(windows)]
fn with_extensions(dir: &std::path::Path, name: &str, path: &std::path::Path) -> Vec<PathBuf> {
    if path.extension().is_some() {
        return Vec::new();
    }
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    extensions
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| dir.join(format!("{name}{extension}")))
        .collect()
}

/// Elsewhere programs are run by their exact name.
#[cfg(not(windows))]
fn with_extensions(_dir: &std::path::Path, _name: &str, _path: &std::path::Path) -> Vec<PathBuf> {
    Vec::new()
}

/// Whether `path` is a file that can be run.
fn is_program(path: &std::path::Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
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
    /// `None` once handed over with [`Pty::take_writer`].
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send + Sync>,
    size: PtySize,
}

impl Pty {
    /// Opens a pseudo-terminal of the given size and starts `command` in it.
    pub fn spawn(command: &ShellCommand, size: PtySize) -> Result<Self> {
        let spawn_error = |message: String| PtyError::Spawn {
            program: command.program.to_string_lossy().into_owned(),
            message,
        };
        let builder = command.to_builder().map_err(spawn_error)?;
        let pair = native_pty_system()
            .openpty(size.into())
            .map_err(|err| PtyError::Open(err.to_string()))?;

        let child = pair
            .slave
            .spawn_command(builder)
            .map_err(|err| spawn_error(err.to_string()))?;
        // The child holds its own handle to the slave side. Releasing ours
        // lets the reader observe end-of-file once the child exits.
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|err| PtyError::Stream(err.to_string()))?;

        Ok(Self {
            master: pair.master,
            writer: Some(writer),
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
    ///
    /// Blocks while the process is not reading its input and the
    /// pseudo-terminal's buffer is full. Fails once the writer was taken
    /// with [`Pty::take_writer`].
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| PtyError::Stream("the writer was taken".into()))?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Takes the writer for the process input, so it can be used on another
    /// thread: writes block while the process is not reading. Returns `None`
    /// if it was already taken.
    pub fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        self.writer.take()
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

    /// A directory holding a runnable file called `name`.
    fn dir_with_program(label: &str, name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("woop-pty-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let file = dir.join(name);
        std::fs::write(&file, "").expect("write program");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        dir
    }

    fn resolve(
        program: &str,
        search_path: Option<&std::path::Path>,
    ) -> std::result::Result<PathBuf, String> {
        resolve_program_in(
            std::ffi::OsStr::new(program),
            search_path.map(|dir| dir.as_os_str().to_owned()),
        )
    }

    #[test]
    fn bare_names_are_found_in_the_search_path_only() {
        let dir = dir_with_program("found", "woop-test-shell.exe");
        assert_eq!(
            resolve("woop-test-shell.exe", Some(&dir)),
            Ok(dir.join("woop-test-shell.exe"))
        );
        // A name that is not in the search path is never run from anywhere
        // else, such as the current directory, where a program of the same
        // name could have been planted.
        assert!(resolve("woop-test-shell.exe", None).is_err());
        let current = std::env::current_dir().expect("current dir");
        let relative = PathBuf::from(".");
        assert!(resolve("woop-test-shell.exe", Some(&relative)).is_err());
        assert!(resolve("woop-test-shell.exe", Some(&current.join("missing"))).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_without_extension_use_pathext() {
        let dir = dir_with_program("pathext", "woop-test-ext.exe");
        assert_eq!(
            resolve("woop-test-ext", Some(&dir))
                .map(|path| path.file_name().map(|name| name.to_ascii_lowercase())),
            Ok(Some("woop-test-ext.exe".into()))
        );
    }

    #[test]
    fn relative_paths_are_refused_and_absolute_ones_kept() {
        let dir = dir_with_program("paths", "woop-test-abs");
        assert!(resolve("../woop-test-abs", Some(&dir)).is_err());
        assert!(resolve("bin/woop-test-abs", Some(&dir)).is_err());
        assert!(resolve("", Some(&dir)).is_err());
        let absolute = dir.join("woop-test-abs");
        assert_eq!(
            resolve(&absolute.to_string_lossy(), None),
            Ok(absolute.clone())
        );
    }

    #[test]
    fn spawning_an_unknown_program_fails_without_running_anything() {
        let command = ShellCommand::new("woop-no-such-program-anywhere");
        let error = Pty::spawn(&command, PtySize::new(24, 80)).expect_err("spawn must fail");
        assert!(error.to_string().contains("not found in PATH"), "{error}");
    }
}
