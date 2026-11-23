//! Loading Woop's shell integration into the user's shell.
//!
//! The integration scripts in `shell/` make the shell emit OSC 133 marks
//! around the prompt, the command and its result, which [`Terminal`] turns
//! into command blocks. They wrap the user's own prompt and configuration,
//! which still load as usual.
//!
//! - PowerShell receives its script inline with `-EncodedCommand`.
//! - fish receives its script inline with `--init-command`.
//! - bash and zsh read files, which are written to a per-user directory
//!   each time a shell starts so that they always match this version.
//!
//! [`Terminal`]: crate::Terminal

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use woop_pty::ShellCommand;

use crate::{Session, TerminalSize};

const POWERSHELL: &str = include_str!("../shell/woop.ps1");
const BASH: &str = include_str!("../shell/woop.bash");
const FISH: &str = include_str!("../shell/woop.fish");
const ZSH_FILES: [(&str, &str); 3] = [
    (".zshenv", include_str!("../shell/zsh/.zshenv")),
    (".zprofile", include_str!("../shell/zsh/.zprofile")),
    (".zshrc", include_str!("../shell/zsh/.zshrc")),
];

/// Shells Woop has an integration for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// Windows PowerShell or PowerShell 7.
    PowerShell,
    /// bash.
    Bash,
    /// zsh.
    Zsh,
    /// fish.
    Fish,
}

impl Shell {
    /// Recognises a shell from the program name, such as `/bin/zsh` or
    /// `pwsh.exe`.
    pub fn detect(program: &OsStr) -> Option<Self> {
        // Split on both separators so Windows paths are understood
        // everywhere, and ignore the extension (`.exe`).
        let file = program.to_str()?.rsplit(['/', '\\']).next()?;
        let name = file.split('.').next()?.to_ascii_lowercase();
        match name.as_str() {
            "pwsh" | "powershell" => Some(Self::PowerShell),
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

/// Environment variable that passes the mark token to the integration
/// scripts, which remove it from the environment straight away.
const TOKEN_VARIABLE: &str = "WOOP_TOKEN";

/// A secret shared with the integration scripts of one session, which add
/// it to every mark so that output cannot forge them (ADR 0018).
#[derive(Clone, PartialEq, Eq)]
pub struct MarkToken(String);

impl MarkToken {
    /// Generates a new random token: 128 bits as 32 hexadecimal digits.
    ///
    /// The bits come from the standard library's SipHash keyed with random
    /// keys from the operating system. Nobody sees any output of that hash
    /// before the token is used, so it cannot be predicted.
    pub fn generate() -> Self {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        let half = |part: u8| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u8(part);
            hasher.write_u128(nanos);
            hasher.write_u32(std::process::id());
            hasher.finish()
        };
        Self(format!("{:016x}{:016x}", half(0), half(1)))
    }

    /// The token as text, as the scripts write it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for MarkToken {
    /// Never prints the secret, so it cannot end up in logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MarkToken(..)")
    }
}

/// Starts `command` with Woop's integration, signed with a new token that
/// the session requires before it is returned (ADR 0018, 0019): marks
/// without it are ignored from the first byte of output. `notify` is called
/// as in [`Session::spawn_with_notifier`].
pub fn spawn(
    command: ShellCommand,
    size: TerminalSize,
    notify: impl Fn() + Send + Sync + 'static,
) -> woop_pty::Result<Session> {
    let token = MarkToken::generate();
    let mut session = Session::spawn_with_notifier(&apply(command, &token), size, notify)?;
    session.terminal_mut().require_mark_token(&token);
    Ok(session)
}

/// Returns `command` changed so that the shell loads Woop's integration,
/// which signs its marks with `token`. Prefer [`spawn`], which also makes
/// the session require the token.
///
/// Commands for unknown shells, or for which the integration files cannot
/// be written, are returned unchanged: the shell then works as in any
/// terminal, without command blocks.
pub fn apply(command: ShellCommand, token: &MarkToken) -> ShellCommand {
    let Some(shell) = Shell::detect(&command.program) else {
        return command;
    };
    let with_token = |command: ShellCommand| command.env(TOKEN_VARIABLE, token.as_str());
    match shell {
        Shell::PowerShell => with_token(command)
            .arg("-NoExit")
            .arg("-EncodedCommand")
            .arg(encode_powershell(POWERSHELL)),
        Shell::Fish => with_token(command).arg("--init-command").arg(FISH),
        Shell::Bash => match write_files(&[("woop.bash", BASH)]) {
            Some(dir) => with_token(command)
                .arg("--rcfile")
                .arg(dir.join("woop.bash")),
            None => command,
        },
        Shell::Zsh => match write_files(&ZSH_FILES) {
            Some(dir) => {
                let user_dir = std::env::var_os("ZDOTDIR")
                    .or_else(|| std::env::var_os("HOME"))
                    .unwrap_or_default();
                with_token(command)
                    .env("WOOP_USER_ZDOTDIR", user_dir)
                    .env("ZDOTDIR", dir)
            }
            None => command,
        },
    }
}

/// Writes integration files into the per-user directory and returns it.
fn write_files(files: &[(&str, &str)]) -> Option<PathBuf> {
    let dir = integration_dir()?;
    write_files_in(&dir, files).ok()?;
    Some(dir)
}

/// Writes `files` into `dir`, creating it if needed. On Unix the directory
/// and the files are made private to the user (modes 700 and 600), also
/// when they already existed with wider permissions: the shell runs these
/// files at start-up, so nobody else may be able to change them.
fn write_files_in(dir: &Path, files: &[(&str, &str)]) -> io::Result<()> {
    create_private_dir(dir)?;
    for (name, contents) in files {
        let path = dir.join(name);
        // Skip the write when the file is already up to date.
        if fs::read_to_string(&path).ok().as_deref() != Some(*contents) {
            write_private(&path, contents)?;
        }
        make_private(&path, 0o600)?;
    }
    Ok(())
}

/// Writes a file that is private from the moment it is created on Unix.
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    io::Write::write_all(&mut options.open(path)?, contents.as_bytes())
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)?;
    make_private(dir, 0o700)
}

/// Restricts `path` to its owner on Unix. Elsewhere, files in the user's
/// profile are already private to the user.
#[cfg_attr(not(unix), allow(unused_variables))]
fn make_private(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

/// Directory for integration files: inside the user's cache directory, so
/// that other users cannot change what the shell loads.
fn integration_dir() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let home = std::env::var_os("HOME").filter(|dir| !dir.is_empty())?;
            Some(PathBuf::from(home).join(".cache"))
        })?;
    Some(cache.join("woop").join("shell-integration"))
}

/// Encodes a script for PowerShell's `-EncodedCommand`: Base64 of its
/// UTF-16LE bytes.
fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

/// Standard Base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |group, (i, &byte)| {
            group | u32::from(byte) << (16 - 8 * i)
        });
        for i in 0..4 {
            if i <= chunk.len() {
                let index = (group >> (18 - 6 * i)) & 0x3f;
                out.push(char::from(ALPHABET[index as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shells_by_program_name() {
        let detect = |program: &str| Shell::detect(OsStr::new(program));
        assert_eq!(detect("pwsh.exe"), Some(Shell::PowerShell));
        assert_eq!(
            detect(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            Some(Shell::PowerShell)
        );
        assert_eq!(detect("/bin/bash"), Some(Shell::Bash));
        assert_eq!(detect("/usr/bin/zsh"), Some(Shell::Zsh));
        assert_eq!(detect("/opt/homebrew/bin/fish"), Some(Shell::Fish));
        assert_eq!(detect("/bin/sh"), None);
        assert_eq!(detect("cmd.exe"), None);
    }

    fn token_variable(command: &ShellCommand) -> Option<String> {
        command
            .env
            .iter()
            .find(|(key, _)| key == TOKEN_VARIABLE)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    }

    #[test]
    fn unknown_shells_are_unchanged() {
        let command = ShellCommand::new("cmd.exe").arg("/Q");
        assert_eq!(apply(command.clone(), &MarkToken::generate()), command);
    }

    #[test]
    fn powershell_receives_the_script_inline_and_the_token_privately() {
        let token = MarkToken::generate();
        let command = apply(ShellCommand::new("pwsh.exe").arg("-NoLogo"), &token);
        let args: Vec<_> = command.args.iter().map(|a| a.to_string_lossy()).collect();
        assert_eq!(args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
        assert_eq!(args[3], encode_powershell(POWERSHELL));
        // In the environment, never on the command line.
        assert_eq!(token_variable(&command).as_deref(), Some(token.as_str()));
        assert!(!args.iter().any(|arg| arg.contains(token.as_str())));
    }

    #[test]
    fn fish_receives_the_script_inline() {
        let token = MarkToken::generate();
        let command = apply(ShellCommand::new("fish"), &token);
        assert_eq!(command.args, ["--init-command", FISH]);
        assert_eq!(token_variable(&command).as_deref(), Some(token.as_str()));
    }

    #[test]
    fn tokens_are_long_random_and_never_printed() {
        let first = MarkToken::generate();
        let second = MarkToken::generate();
        assert_eq!(first.as_str().len(), 32);
        assert!(first.as_str().bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
        assert_eq!(format!("{first:?}"), "MarkToken(..)");
    }

    #[test]
    fn the_scripts_read_the_token_from_the_environment() {
        // The token reaches the shell only through its environment; the
        // scripts are shared by every session and written to disk.
        for script in [POWERSHELL, BASH, FISH, ZSH_FILES[0].1] {
            assert!(script.contains(TOKEN_VARIABLE));
        }
    }

    #[test]
    fn writes_and_updates_integration_files() {
        let dir = std::env::temp_dir().join(format!("woop-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        write_files_in(&dir, &[("a", "one")]).expect("write");
        write_files_in(&dir, &[("a", "two")]).expect("update");
        assert_eq!(fs::read_to_string(dir.join("a")).expect("read"), "two");

        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[cfg(unix)]
    #[test]
    fn integration_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("woop-private-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // A directory and file left with permissions others can use.
        fs::create_dir_all(&dir).expect("create");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).expect("chmod dir");
        fs::write(dir.join("a"), "old").expect("write");
        fs::set_permissions(dir.join("a"), fs::Permissions::from_mode(0o666)).expect("chmod");

        write_files_in(&dir, &[("a", "new"), ("b", "other")]).expect("write");

        let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("a")), 0o600);
        assert_eq!(mode(&dir.join("b")), 0o600);

        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode_powershell("ab"), "YQBiAA==");
    }
}
