//! End-to-end tests of the shell integration scripts with real shells.

use std::time::Duration;

use woop_term::{BlockState, Session, ShellCommand, TerminalSize, input, shell_integration};

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
    let command = session
        .terminal()
        .block_command(block.id())
        .unwrap_or_default();
    (command, block.exit_code())
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
    submit_from_editor(&mut session);
}

/// The shell reports that it waits for a command with an empty line, so
/// Woop's editor can take over, and runs what the editor submits.
fn submit_from_editor(session: &mut Session) {
    wait_for_editor(session);
    let before = finished(session);
    let mode = session.terminal().mode();
    session
        .write(&input::encode_command("echo from-editor", mode))
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
        "the editor's command did not run; screen was:\n{}",
        session.terminal().screen_text()
    );
    let screen = session.terminal().screen_text();
    assert!(
        screen.lines().any(|line| line.trim_end() == "from-editor"),
        "screen was:\n{screen}"
    );
    wait_for_editor(session);
}

fn wait_for_editor(session: &mut Session) {
    let waiting = session
        .wait_until(TIMEOUT, |term| term.awaiting_command().is_some())
        .expect("pump");
    assert!(
        waiting,
        "the shell never waited for a command with an empty line; screen was:\n{}",
        session.terminal().screen_text()
    );
}

/// Woop's editor panel takes rows from the grid when it appears, so the
/// terminal is resized at the prompt; the shell must still be seen waiting
/// for a command afterwards.
#[cfg(windows)]
#[test]
fn powershell_still_awaits_a_command_after_a_resize_at_the_prompt() {
    let mut session = start(
        ShellCommand::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile"),
    );
    wait_for_editor(&mut session);
    session.resize(TerminalSize::new(22, 100)).expect("resize");
    // Let the shell and the pseudo-terminal react to the resize.
    let _ = session
        .wait_until(Duration::from_secs(2), |_| false)
        .expect("pump");
    let id = session.terminal().awaiting_command();
    let blocks = session.terminal().blocks().len();
    assert!(
        id.is_some(),
        "not awaiting after resize ({blocks} blocks, cursor {:?}); screen was:\n{}",
        session.terminal().cursor(),
        session.terminal().screen_text()
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

/// The user's own PROMPT_COMMAND must still see the last exit status.
#[cfg(unix)]
#[test]
fn bash_keeps_the_exit_status_for_the_users_prompt_command() {
    let home = empty_home("bash-status");
    std::fs::write(
        home.join(".bashrc"),
        "PROMPT_COMMAND='echo \"woop-status=$?\"'\n",
    )
    .expect("write .bashrc");
    let mut session = start(ShellCommand::new("bash").env("HOME", &home));

    run_block(&mut session, "sh -c 'exit 3'");

    let screen = session.terminal().screen_text();
    assert!(screen.contains("woop-status=3"), "screen was:\n{screen}");
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
