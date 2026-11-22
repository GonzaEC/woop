//! A shell running in a pseudo-terminal, connected to a [`Terminal`].

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use woop_pty::{ExitStatus, Pty, Result, ShellCommand};

use crate::terminal::{Terminal, TerminalEvent, TerminalSize};

const READ_BUFFER_SIZE: usize = 32 * 1024;

/// Maximum number of output chunks waiting between the reader thread and
/// the owner. With reads of at most [`READ_BUFFER_SIZE`] bytes this bounds
/// buffered output to 1 MiB. When the queue is full the reader stops
/// reading, so a process that floods the terminal blocks on its own writes
/// instead of making Woop's memory grow.
const OUTPUT_QUEUE_CHUNKS: usize = 32;

/// Output applied by one [`Session::pump`] stops once this many bytes have
/// been applied (so at most this plus one chunk), so that a process
/// producing output faster than it can be parsed never keeps the owner busy
/// for long: a frame can be drawn between pumps. Remaining output triggers
/// another notification.
const BYTES_PER_PUMP: usize = 128 * 1024;

type Notify = Arc<dyn Fn() + Send + Sync>;

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
    pending: Arc<AtomicBool>,
    notify: Notify,
    closed: bool,
}

impl Session {
    /// Starts `command` in a new pseudo-terminal of the given size.
    pub fn spawn(command: &ShellCommand, size: TerminalSize) -> Result<Self> {
        Self::spawn_with_notifier(command, size, || {})
    }

    /// Starts `command` like [`Session::spawn`], calling `notify` from the
    /// reader thread when new output is waiting to be pumped.
    ///
    /// Notifications are coalesced: after one call, `notify` is not called
    /// again until [`Session::pump`] has run. It is also called once when the
    /// output closes. This lets an event loop sleep until there is work.
    pub fn spawn_with_notifier(
        command: &ShellCommand,
        size: TerminalSize,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        // The terminal clamps the size; the process must see the same one.
        let terminal = Terminal::new(size);
        let pty = Pty::spawn(command, terminal.size().into())?;
        let reader = pty.take_reader()?;
        let pending = Arc::new(AtomicBool::new(false));
        let notify: Notify = Arc::new(notify);
        let (output, reader) = start_reader(reader, Arc::clone(&pending), Arc::clone(&notify))?;

        Ok(Self {
            pty,
            terminal,
            output,
            reader: Some(reader),
            pending,
            notify,
            closed: false,
        })
    }

    /// Returns the terminal state.
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Returns the terminal state for changes that do not involve the
    /// process, such as scrolling the view.
    pub fn terminal_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }

    /// Sends input to the process, as if typed on a keyboard.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.pty.write_all(bytes)
    }

    /// Resizes both the pseudo-terminal and the screen.
    pub fn resize(&mut self, size: TerminalSize) -> Result<()> {
        // The terminal clamps the size; the process must see the same one.
        self.terminal.resize(size);
        self.pty.resize(self.terminal.size().into())
    }

    /// Applies output received so far to the terminal, without blocking.
    ///
    /// At most a fixed amount of output is applied per call, so a process
    /// that floods the terminal cannot keep the caller busy; if more is
    /// waiting, the notifier is called again so the caller pumps once more
    /// after handling other work (such as drawing a frame).
    ///
    /// Replies requested by the process (such as cursor position reports)
    /// are written back automatically. The remaining events are returned.
    pub fn pump(&mut self) -> Result<Vec<TerminalEvent>> {
        // Clear the flag before draining, so output that arrives meanwhile
        // triggers a fresh notification instead of being missed.
        self.pending.store(false, Ordering::Release);
        let terminal = &mut self.terminal;
        match drain(&self.output, BYTES_PER_PUMP, |bytes| {
            terminal.advance(bytes)
        }) {
            Drained::Empty => {}
            Drained::Closed => self.closed = true,
            Drained::Limit => {
                if !self.pending.swap(true, Ordering::AcqRel) {
                    (self.notify)();
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

/// Starts the thread that reads the process output into a bounded queue.
fn start_reader(
    reader: Box<dyn Read + Send>,
    pending: Arc<AtomicBool>,
    notify: Notify,
) -> std::io::Result<(Receiver<Vec<u8>>, JoinHandle<()>)> {
    let (sender, output) = mpsc::sync_channel(OUTPUT_QUEUE_CHUNKS);
    let handle = thread::Builder::new()
        .name("woop-pty-reader".into())
        .spawn(move || read_output(reader, sender, &pending, &*notify))?;
    Ok((output, handle))
}

/// Reads output until the process closes it or the owner goes away.
///
/// `send` blocks while the queue is full, which stops reading and applies
/// back-pressure to the process.
fn read_output(
    mut reader: Box<dyn Read + Send>,
    sender: SyncSender<Vec<u8>>,
    pending: &AtomicBool,
    notify: &(dyn Fn() + Send + Sync),
) {
    let mut buffer = vec![0u8; READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if sender.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
                if !pending.swap(true, Ordering::AcqRel) {
                    notify();
                }
            }
        }
    }
    // Disconnect the channel before the final notification, so the owner
    // observes the closed output when it pumps.
    drop(sender);
    notify();
}

/// Why [`drain`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drained {
    /// No more output is waiting.
    Empty,
    /// The limit was reached; more output may be waiting.
    Limit,
    /// The output is closed and fully consumed.
    Closed,
}

/// Applies chunks from `output` until at least `budget` bytes were applied.
fn drain(output: &Receiver<Vec<u8>>, budget: usize, mut apply: impl FnMut(&[u8])) -> Drained {
    let mut applied = 0;
    while applied < budget {
        match output.try_recv() {
            Ok(bytes) => {
                applied += bytes.len();
                apply(&bytes);
            }
            Err(TryRecvError::Empty) => return Drained::Empty,
            Err(TryRecvError::Disconnected) => return Drained::Closed,
        }
    }
    Drained::Limit
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A process that never stops writing.
    struct Flood {
        reads: Arc<AtomicUsize>,
    }

    impl Read for Flood {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let len = buffer.len().min(16);
            buffer[..len].fill(b'x');
            Ok(len)
        }
    }

    #[test]
    fn the_reader_stops_when_output_is_not_consumed() {
        let reads = Arc::new(AtomicUsize::new(0));
        let flood = Box::new(Flood {
            reads: Arc::clone(&reads),
        });
        let (output, reader) =
            start_reader(flood, Arc::new(AtomicBool::new(false)), Arc::new(|| {}))
                .expect("start reader");

        thread::sleep(Duration::from_millis(300));
        let read = reads.load(Ordering::SeqCst);
        // One chunk per queue slot, plus the one waiting to be sent.
        assert!(
            read <= OUTPUT_QUEUE_CHUNKS + 1,
            "{read} reads were buffered without being consumed"
        );

        // The reader thread ends once the owner goes away.
        drop(output);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !reader.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(reader.is_finished());
    }

    #[test]
    fn drain_applies_a_bounded_amount_of_output() {
        let (sender, output) = mpsc::channel();
        for _ in 0..5 {
            sender.send(vec![b'x'; 10]).expect("send");
        }
        let mut applied = 0;

        // The budget is in bytes: 25 bytes means three chunks of 10.
        assert_eq!(drain(&output, 25, |b| applied += b.len()), Drained::Limit);
        assert_eq!(applied, 30);
        assert_eq!(drain(&output, 25, |b| applied += b.len()), Drained::Empty);
        assert_eq!(applied, 50);

        drop(sender);
        assert_eq!(drain(&output, 25, |b| applied += b.len()), Drained::Closed);
    }
}
