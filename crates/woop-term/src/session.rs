//! A shell running in a pseudo-terminal, connected to a [`Terminal`].

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use woop_pty::{ExitStatus, Pty, PtyError, Result, ShellCommand};

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

/// Most input waiting for the process at once. Writes are done on a
/// separate thread, because they block while the process is not reading;
/// beyond this backlog, new input is refused instead of piling up.
const MAX_PENDING_INPUT: usize = 16 * 1024 * 1024;

/// Most bytes of replies to terminal queries written per pump. Replies are
/// produced by program output, so a flood of queries must not turn into an
/// unbounded amount of input; replies past the budget are dropped.
const REPLY_BYTES_PER_PUMP: usize = 16 * 1024;

type Notify = Arc<dyn Fn() + Send + Sync>;

/// Input for the process, written by a dedicated thread so the owner never
/// blocks on a process that does not read.
struct InputQueue {
    sender: mpsc::Sender<Vec<u8>>,
    /// Bytes sent to the writer thread and not yet written.
    pending: Arc<AtomicUsize>,
}

impl InputQueue {
    fn start(mut writer: Box<dyn Write + Send>) -> std::io::Result<Self> {
        let (sender, chunks) = mpsc::channel::<Vec<u8>>();
        let pending = Arc::new(AtomicUsize::new(0));
        let written = Arc::clone(&pending);
        thread::Builder::new()
            .name("woop-pty-writer".into())
            .spawn(move || {
                // Ends when the session is dropped (the channel closes) or the
                // process input is closed (a write fails).
                for chunk in chunks {
                    let result = writer.write_all(&chunk).and_then(|()| writer.flush());
                    written.fetch_sub(chunk.len(), Ordering::AcqRel);
                    if result.is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self { sender, pending })
    }

    /// Queues `bytes` without blocking, or refuses them if the backlog is
    /// full.
    fn send(&self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let len = bytes.len();
        let before = self.pending.fetch_add(len, Ordering::AcqRel);
        if before + len > MAX_PENDING_INPUT {
            self.pending.fetch_sub(len, Ordering::AcqRel);
            return Err(PtyError::InputBacklog(len));
        }
        self.sender.send(bytes.to_vec()).map_err(|_| {
            self.pending.fetch_sub(len, Ordering::AcqRel);
            PtyError::Stream("the process input is closed".into())
        })
    }
}

/// Separates replies from other events, keeping at most `budget` bytes of
/// replies.
fn split_replies(events: Vec<TerminalEvent>, budget: usize) -> (Vec<u8>, Vec<TerminalEvent>) {
    let mut replies = Vec::new();
    let mut others = Vec::new();
    for event in events {
        match event {
            TerminalEvent::Reply(reply) => {
                if replies.len() + reply.len() <= budget {
                    replies.extend_from_slice(reply.as_bytes());
                }
            }
            other => others.push(other),
        }
    }
    (replies, others)
}

/// A process running in a pseudo-terminal, together with its screen state.
///
/// Output is read on a dedicated thread so a busy process never blocks the
/// caller. Call [`Session::pump`] to apply pending output to the
/// [`Terminal`].
pub struct Session {
    pty: Pty,
    input: InputQueue,
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
        let mut pty = Pty::spawn(command, terminal.size().into())?;
        let reader = pty.take_reader()?;
        let writer = pty
            .take_writer()
            .ok_or_else(|| PtyError::Stream("the writer was taken".into()))?;
        let input = InputQueue::start(writer)?;
        let pending = Arc::new(AtomicBool::new(false));
        let notify: Notify = Arc::new(notify);
        let (output, reader) = start_reader(reader, Arc::clone(&pending), Arc::clone(&notify))?;

        Ok(Self {
            pty,
            input,
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
    ///
    /// Never blocks: input is written by a separate thread. If the process
    /// has stopped reading and too much input is already waiting, the input
    /// is refused with [`PtyError::InputBacklog`].
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.input.send(bytes)
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
        let (replies, events) = split_replies(self.terminal.take_events(), REPLY_BYTES_PER_PUMP);
        // Replies are best effort: a process that floods queries and does
        // not read the answers loses them rather than stalling the owner.
        let _ = self.input.send(&replies);
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
    fn input_backlog_is_bounded_and_never_blocks() {
        /// A process that never reads: every write blocks forever.
        struct Stuck(Receiver<()>);
        impl Write for Stuck {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                let _ = self.0.recv();
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let (_keep_blocked, blocked) = mpsc::channel();
        let input = InputQueue::start(Box::new(Stuck(blocked))).expect("start writer");
        let chunk = vec![b'x'; 1024 * 1024];
        let start = Instant::now();
        let mut accepted = 0;
        for _ in 0..20 {
            if input.send(&chunk).is_ok() {
                accepted += chunk.len();
            }
        }

        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(accepted <= MAX_PENDING_INPUT);
        assert!(matches!(
            input.send(&chunk),
            Err(PtyError::InputBacklog(len)) if len == chunk.len()
        ));
    }

    #[test]
    fn replies_are_limited_per_pump() {
        let events = vec![
            TerminalEvent::Reply("12345".into()),
            TerminalEvent::Bell,
            TerminalEvent::Reply("678".into()),
            TerminalEvent::Reply("9".into()),
        ];

        let (replies, others) = split_replies(events, 6);

        assert_eq!(replies, b"123459");
        assert_eq!(others, [TerminalEvent::Bell]);
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
