//! End-to-end tests of the shell integration scripts with real shells.

use std::time::Duration;

use woop_term::{BlockState, Session, ShellCommand, TerminalSize, shell_integration};

const TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `command` in `shell` with the integration loaded and returns the
/// finished block's command text and exit code.
fn run_block(session: &mut Session, command: &str) -> (String, Option<i32>) {
    let before = finished(session);
    session
        .write(format!("{command}\r").as_bytes())
        .expect("write");
    let done = session
        .wait_until(TIMEOUT, |term| {
            term.blocks()
                .iter()
                .filter(|block| block.state() == BlockState::Finished)
                .count()
                > before
        })
        .expect("pump");
    assert!(
        done,
        "no block finished for {command:?}; screen was:\n{}",
        session.terminal().screen_text()
    );
    let block = session
        .terminal()
        .blocks()
        .iter()
        .rfind(|block| block.state() == BlockState::Finished)
        .expect("finished block");
    (block.command().to_owned(), block.exit_code())
}

fn finished(session: &Session) -> usize {
    session
        .terminal()
        .blocks()
        .iter()
        .filter(|block| block.state() == BlockState::Finished)
        .count()
}

fn start(command: ShellCommand) -> Session {
    let mut session = Session::spawn(
        &shell_integration::apply(command),
        TerminalSize::new(24, 100),
    )
    .expect("spawn");
    let ready = session
        .wait_until(TIMEOUT, |term| {
            term.blocks()
                .last()
                .is_some_and(|block| block.state() == BlockState::Prompt)
        })
        .expect("pump");
    assert!(
        ready,
        "no prompt was marked; screen was:\n{}",
        session.terminal().screen_text()
    );
    session
}

fn check_shell(command: ShellCommand, failing: &str, failing_code: i32) {
    let mut session = start(command);

    assert_eq!(
        run_block(&mut session, "echo woop"),
        ("echo woop".to_owned(), Some(0))
    );
    assert_eq!(
        run_block(&mut session, failing),
        (failing.to_owned(), Some(failing_code))
    );
}

#[cfg(windows)]
#[test]
fn powershell_marks_blocks() {
    check_shell(
        ShellCommand::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile"),
        "cmd /c exit 3",
        3,
    );
}

#[cfg(unix)]
#[test]
fn bash_marks_blocks() {
    check_shell(
        ShellCommand::new("bash").env("HOME", empty_home("bash")),
        "sh -c 'exit 3'",
        3,
    );
}

#[cfg(unix)]
#[test]
fn zsh_marks_blocks() {
    if !in_path("zsh") {
        eprintln!("zsh is not installed; skipping");
        return;
    }
    check_shell(
        ShellCommand::new("zsh").env("HOME", empty_home("zsh")),
        "sh -c 'exit 3'",
        3,
    );
}

/// A fresh home directory, so the user's own configuration does not affect
/// the test.
#[cfg(unix)]
fn empty_home(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("woop-integration-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create home");
    dir
}

#[cfg(unix)]
fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}
