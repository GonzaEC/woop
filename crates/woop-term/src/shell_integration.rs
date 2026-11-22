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
use std::path::PathBuf;

use woop_pty::ShellCommand;

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

/// Returns `command` changed so that the shell loads Woop's integration.
///
/// Commands for unknown shells, or for which the integration files cannot
/// be written, are returned unchanged: the shell then works as in any
/// terminal, without command blocks.
pub fn apply(command: ShellCommand) -> ShellCommand {
    let Some(shell) = Shell::detect(&command.program) else {
        return command;
    };
    match shell {
        Shell::PowerShell => command
            .arg("-NoExit")
            .arg("-EncodedCommand")
            .arg(encode_powershell(POWERSHELL)),
        Shell::Fish => command.arg("--init-command").arg(FISH),
        Shell::Bash => match write_files(&[("woop.bash", BASH)]) {
            Some(dir) => command.arg("--rcfile").arg(dir.join("woop.bash")),
            None => command,
        },
        Shell::Zsh => match write_files(&ZSH_FILES) {
            Some(dir) => {
                let user_dir = std::env::var_os("ZDOTDIR")
                    .or_else(|| std::env::var_os("HOME"))
                    .unwrap_or_default();
                command
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
    std::fs::create_dir_all(&dir).ok()?;
    for (name, contents) in files {
        let path = dir.join(name);
        // Skip the write when the file is already up to date.
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*contents) {
            std::fs::write(&path, contents).ok()?;
        }
    }
    Some(dir)
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

    #[test]
    fn unknown_shells_are_unchanged() {
        let command = ShellCommand::new("cmd.exe").arg("/Q");
        assert_eq!(apply(command.clone()), command);
    }

    #[test]
    fn powershell_receives_the_script_inline() {
        let command = apply(ShellCommand::new("pwsh.exe").arg("-NoLogo"));
        let args: Vec<_> = command.args.iter().map(|a| a.to_string_lossy()).collect();
        assert_eq!(args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
        assert_eq!(args[3], encode_powershell(POWERSHELL));
    }

    #[test]
    fn fish_receives_the_script_inline() {
        let command = apply(ShellCommand::new("fish"));
        assert_eq!(command.args, ["--init-command", FISH]);
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
