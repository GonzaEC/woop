//! End-to-end tests of the shell integration scripts with real shells.

use std::time::Duration;

use woop_term::shell_integration;
use woop_term::{BlockState, Session, ShellCommand, TerminalSize, input};

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

/// Starts `command` as Woop does, with the integration loaded and only
/// marks carrying the session's token accepted: blocks then prove that the
/// script signs every mark.
fn start(command: ShellCommand) -> Session {
    let mut session =
        shell_integration::spawn(command, TerminalSize::new(24, 100), || {}).expect("spawn");
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

/// `print_token` prints the `WOOP_TOKEN` environment variable between
/// brackets, as a program started from the shell would see it.
fn check_shell(command: ShellCommand, failing: &str, failing_code: i32, print_token: &str) {
    let mut session = start(command);

    assert_eq!(
        run_block(&mut session, "echo woop"),
        ("echo woop".to_owned(), Some(0))
    );
    assert_eq!(
        run_block(&mut session, failing),
        (failing.to_owned(), Some(failing_code))
    );
    // The script removed the token from the environment, so programs do
    // not inherit it.
    run_block(&mut session, print_token);
    let screen = session.terminal().screen_text();
    assert!(
        screen.lines().any(|line| line.trim_end() == "[]"),
        "the token reached the environment; screen was:\n{screen}"
    );
    reports_its_directory(&mut session);
    submit_from_editor(&mut session);
}

/// After `cd`, the shell reports its new directory with the next prompt,
/// even with characters that need encoding (ADR 0022).
fn reports_its_directory(session: &mut Session) {
    let directory = std::env::temp_dir().join(format!("woop dir;ñ%-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create directory");
    let expected = directory.canonicalize().expect("canonical directory");
    // Single quotes keep spaces and `;` literal in PowerShell, bash and zsh.
    run_block(session, &format!("cd '{}'", directory.display()));
    let reported = session
        .wait_until(TIMEOUT, |term| {
            term.current_directory()
                .and_then(|reported| std::path::Path::new(reported).canonicalize().ok())
                .is_some_and(|reported| reported == expected)
        })
        .expect("pump");
    assert!(
        reported,
        "reported {:?}, expected {}",
        session.terminal().current_directory(),
        directory.display()
    );
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
/// for a command afterwards, and run what the editor submits.
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
    // Matches on the command's output rather than waiting a fixed time for
    // the shell to react to the resize.
    submit_from_editor(&mut session);
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
        "echo \"[$env:WOOP_TOKEN]\"",
    );
}

#[cfg(unix)]
#[test]
fn bash_marks_blocks() {
    check_shell(
        ShellCommand::new("bash").env("HOME", empty_home("bash")),
        "sh -c 'exit 3'",
        3,
        "echo \"[$WOOP_TOKEN]\"",
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

/// A prompt that already carries its own, unsigned marks (as some prompt
/// themes do) must still be wrapped with signed ones: Woop ignores the
/// unsigned marks, so blocks would otherwise never form.
#[cfg(unix)]
#[test]
fn bash_signs_a_prompt_that_has_its_own_marks() {
    let home = empty_home("bash-own-marks");
    std::fs::write(home.join(".bashrc"), "PS1='\\[\\e]133;A\\a\\]$ '\n").expect("write .bashrc");
    let mut session = start(ShellCommand::new("bash").env("HOME", &home));

    assert_eq!(
        run_block(&mut session, "echo woop"),
        ("echo woop".to_owned(), Some(0))
    );
}

/// Changes made to the prompt after Woop wrapped it (a virtual environment
/// prepends its name) survive the next prompt, which reports the directory.
#[cfg(unix)]
fn keeps_changes_to_the_prompt(command: ShellCommand) {
    let mut session = start(command);
    run_block(&mut session, "PS1=\"(venv)$PS1\"");
    run_block(&mut session, "echo after");
    let screen = session.terminal().screen_text();
    assert!(
        screen
            .lines()
            .filter(|line| line.contains("(venv)"))
            .count()
            >= 2,
        "the prompt lost its change; screen was:\n{screen}"
    );
    reports_its_directory(&mut session);
}

#[cfg(unix)]
#[test]
fn bash_keeps_changes_to_the_prompt() {
    let home = empty_home("bash-venv");
    std::fs::write(home.join(".bashrc"), "PS1='$ '\n").expect("write .bashrc");
    keeps_changes_to_the_prompt(ShellCommand::new("bash").env("HOME", &home));
}

#[cfg(unix)]
#[test]
fn zsh_keeps_changes_to_the_prompt() {
    if !in_path("zsh") {
        eprintln!("zsh is not installed; skipping");
        return;
    }
    let home = empty_home("zsh-venv");
    std::fs::write(home.join(".zshrc"), "PS1='$ '\n").expect("write .zshrc");
    keeps_changes_to_the_prompt(ShellCommand::new("zsh").env("HOME", &home));
}

#[cfg(unix)]
#[test]
fn zsh_signs_a_prompt_that_has_its_own_marks() {
    if !in_path("zsh") {
        eprintln!("zsh is not installed; skipping");
        return;
    }
    let home = empty_home("zsh-own-marks");
    std::fs::write(home.join(".zshrc"), "PS1=$'%{\\e]133;A\\a%}$ '\n").expect("write .zshrc");
    let mut session = start(ShellCommand::new("zsh").env("HOME", &home));

    assert_eq!(
        run_block(&mut session, "echo woop"),
        ("echo woop".to_owned(), Some(0))
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
        "echo \"[$WOOP_TOKEN]\"",
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
