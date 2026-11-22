//! End-to-end tests that run real processes in a pseudo-terminal.

use std::time::Duration;

use woop_term::{Session, ShellCommand, TerminalSize};

const TIMEOUT: Duration = Duration::from_secs(20);

fn size() -> TerminalSize {
    TerminalSize::new(24, 80)
}

/// A command that prints `text` and exits.
fn print_command(text: &str) -> ShellCommand {
    if cfg!(windows) {
        ShellCommand::new("cmd.exe")
            .arg("/C")
            .arg(format!("echo {text}"))
    } else {
        ShellCommand::new("/bin/sh")
            .arg("-c")
            .arg(format!("echo {text}"))
    }
}

/// An interactive shell with a predictable prompt.
fn interactive_shell() -> ShellCommand {
    if cfg!(windows) {
        ShellCommand::new("cmd.exe").arg("/Q").arg("/K")
    } else {
        ShellCommand::new("/bin/sh").env("PS1", "$ ")
    }
}

#[test]
fn captures_command_output() {
    let mut session = Session::spawn(&print_command("woop-output"), size()).expect("spawn");

    let found = session
        .wait_until(TIMEOUT, |term| term.screen_text().contains("woop-output"))
        .expect("pump");

    assert!(found, "screen was:\n{}", session.terminal().screen_text());
}

#[test]
fn sends_input_to_the_shell() {
    let mut session = Session::spawn(&interactive_shell(), size()).expect("spawn");

    // The input "40+2" never contains the expected result, so finding "42"
    // proves the shell evaluated the command.
    let command: &[u8] = if cfg!(windows) {
        b"set /a 40+2\r"
    } else {
        b"echo $((40+2))\r"
    };
    session.write(command).expect("write");

    let found = session
        .wait_until(TIMEOUT, |term| {
            // The prompt may be printed on the same line as the result
            // when the input is echoed before the shell is ready.
            term.screen_lines()
                .iter()
                .any(|line| line.split_whitespace().last() == Some("42"))
        })
        .expect("pump");

    assert!(found, "screen was:\n{}", session.terminal().screen_text());
}

#[test]
fn resizes_the_terminal() {
    let mut session = Session::spawn(&interactive_shell(), size()).expect("spawn");

    session.resize(TerminalSize::new(10, 40)).expect("resize");

    assert_eq!(session.terminal().size(), TerminalSize::new(10, 40));
    assert_eq!(session.terminal().screen_lines().len(), 10);
}

#[test]
fn notifies_when_output_arrives() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let notify = move || {
        counter.fetch_add(1, Ordering::SeqCst);
    };
    let mut session =
        Session::spawn_with_notifier(&print_command("woop-notify"), size(), notify).expect("spawn");

    let found = session
        .wait_until(TIMEOUT, |term| term.screen_text().contains("woop-notify"))
        .expect("pump");

    assert!(found, "screen was:\n{}", session.terminal().screen_text());
    assert!(calls.load(Ordering::SeqCst) >= 1);
}

/// A process that never reads its input.
fn idle_process() -> ShellCommand {
    if cfg!(windows) {
        ShellCommand::new("ping.exe")
            .arg("-n")
            .arg("30")
            .arg("127.0.0.1")
    } else {
        ShellCommand::new("sleep").arg("30")
    }
}

#[test]
fn writing_never_blocks_the_caller() {
    use std::sync::mpsc;
    use std::time::Instant;

    // Input that the process never reads fills the pseudo-terminal's
    // buffer; writing must still return at once, so the window never hangs.
    let session = Session::spawn(&idle_process(), size()).expect("spawn");
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let mut session = session;
        let start = Instant::now();
        let mut result = Ok(());
        for _ in 0..32 {
            result = result.and(session.write(&[b'x'; 64 * 1024]));
        }
        let _ = done.send((start.elapsed(), result.is_ok()));
    });

    let (elapsed, _) = finished
        .recv_timeout(Duration::from_secs(10))
        .expect("writing 2 MiB blocked the caller");
    assert!(elapsed < Duration::from_secs(2), "writing took {elapsed:?}");
}
