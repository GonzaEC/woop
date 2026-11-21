//! Runs a command in the default shell and prints the resulting screen.
//!
//! ```sh
//! cargo run -p woop-term --example headless -- "echo hello"
//! ```

use std::time::Duration;

use woop_term::{Session, ShellCommand, TerminalSize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let command = if command.is_empty() {
        "echo Hello from Woop".to_owned()
    } else {
        command
    };

    let mut session = Session::spawn(&ShellCommand::default_shell(), TerminalSize::new(24, 100))?;
    session.write(format!("{command}\r").as_bytes())?;

    // Give the shell time to start and run the command, then show the screen.
    session.wait_until(Duration::from_secs(3), |_| false)?;
    println!("{}", session.terminal().screen_text());
    Ok(())
}
