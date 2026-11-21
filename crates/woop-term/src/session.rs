//! A shell running in a pseudo-terminal, connected to a [`Terminal`].

use std::io::Read;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use woop_pty::{ExitStatus, Pty, Result, ShellCommand};

use crate::terminal::{Terminal, TerminalEvent, TerminalSize};

const READ_BUFFER_SIZE: usize = 64 * 1024;

/// A process running in a pseudo-terminal, together with its screen state.
///
/// Output is read on a dedicated thread so a busy process never blocks the
/// caller. Call [`Session::pump`] to apply pending output to the
/// [`Terminal`].
pub struct Session {
    pty: Pty,
    terminal: Terminal,
    output: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    closed: bool,
}

impl Session {
    /// Starts `command` in a new pseudo-terminal of the given size.
    pub fn spawn(command: &ShellCommand, size: TerminalSize) -> Result<Self> {
        let pty = Pty::spawn(command, size.into())?;
        let mut reader = pty.take_reader()?;
        let (sender, output) = mpsc::channel();

        let reader = thread::Builder::new()
            .name("woop-pty-reader".into())
            .spawn(move || {
                let mut buffer = vec![0u8; READ_BUFFER_SIZE];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if sender.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })?;

        Ok(Self {
            pty,
            terminal: Terminal::new(size),
            output,
            reader: Some(reader),
            closed: false,
        })
    }

    /// Returns the terminal state.
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Sends input to the process, as if typed on a keyboard.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.pty.write_all(bytes)
    }

    /// Resizes both the pseudo-terminal and the screen.
    pub fn resize(&mut self, size: TerminalSize) -> Result<()> {
        self.pty.resize(size.into())?;
        self.terminal.resize(size);
        Ok(())
    }

    /// Applies all output received so far to the terminal, without blocking.
    ///
    /// Replies requested by the process (such as cursor position reports)
    /// are written back automatically. The remaining events are returned.
    pub fn pump(&mut self) -> Result<Vec<TerminalEvent>> {
        loop {
            match self.output.try_recv() {
                Ok(bytes) => self.terminal.advance(&bytes),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.closed = true;
                    break;
                }
            }
        }
        self.dispatch_events()
    }

    /// Processes output until `condition` holds or `timeout` elapses.
    ///
    /// Returns whether the condition was met.
    pub fn wait_until(
        &mut self,
        timeout: Duration,
        mut condition: impl FnMut(&Terminal) -> bool,
    ) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump()?;
            if condition(&self.terminal) {
                return Ok(true);
            }
            if self.closed {
                return Ok(false);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            match self.output.recv_timeout(remaining) {
                Ok(bytes) => self.terminal.advance(&bytes),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => self.closed = true,
            }
        }
    }

    /// Returns `true` once the pseudo-terminal output has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Returns the exit status if the process has finished, without blocking.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        self.pty.try_wait()
    }

    /// Terminates the process.
    pub fn kill(&mut self) -> Result<()> {
        self.pty.kill()
    }

    fn dispatch_events(&mut self) -> Result<Vec<TerminalEvent>> {
        let mut events = Vec::new();
        for event in self.terminal.take_events() {
            match event {
                TerminalEvent::Reply(reply) => self.pty.write_all(reply.as_bytes())?,
                other => events.push(other),
            }
        }
        Ok(events)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.kill();
        // The reader thread exits on its own once the pseudo-terminal closes;
        // it is detached rather than joined so dropping never blocks.
        drop(self.reader.take());
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pty", &self.pty)
            .field("terminal", &self.terminal)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
