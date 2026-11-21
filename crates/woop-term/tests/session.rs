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
            term.screen_lines().iter().any(|line| line.trim() == "42")
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
