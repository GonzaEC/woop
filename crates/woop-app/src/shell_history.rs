//! Reads the history file each shell keeps by default (ADR 0023).
//!
//! Read only, never written. The file is bounded, decoded per shell and
//! cleaned of control characters before any command reaches the editor.

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use woop_term::shell_integration::Shell;

/// Most bytes read from the end of a history file.
const MAX_BYTES: u64 = 1024 * 1024;

/// Most commands kept from a history file, the newest ones.
const MAX_COMMANDS: usize = 10_000;

/// Where `shell` keeps its history by default, from the environment
/// variables `var` gives. `windows` selects PowerShell's Windows location.
pub fn path(
    shell: Shell,
    var: impl Fn(&str) -> Option<OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|dir| dir.is_absolute());
    let home = || {
        absolute("HOME").or_else(|| {
            if windows {
                absolute("USERPROFILE")
            } else {
                None
            }
        })
    };
    let data = || absolute("XDG_DATA_HOME").or_else(|| Some(home()?.join(".local").join("share")));
    Some(match shell {
        Shell::Bash => home()?.join(".bash_history"),
        Shell::Zsh => absolute("ZDOTDIR").or_else(home)?.join(".zsh_history"),
        Shell::Fish => data()?.join("fish").join("fish_history"),
        Shell::PowerShell if windows => absolute("APPDATA")?
            .join("Microsoft")
            .join("Windows")
            .join("PowerShell")
            .join("PSReadLine")
            .join("ConsoleHost_history.txt"),
        Shell::PowerShell => data()?
            .join("powershell")
            .join("PSReadLine")
            .join("ConsoleHost_history.txt"),
    })
}

/// Reads `shell`'s history from its default file, oldest command first.
/// Blocks on file I/O, so it runs off the interface thread. Any problem
/// (no file, not a regular file, writable by others) gives no commands.
pub fn load(shell: Shell) -> Vec<String> {
    path(shell, |name| std::env::var_os(name), cfg!(windows))
        .and_then(|path| read_tail(&path).ok())
        .map(|bytes| parse(shell, &bytes))
        .unwrap_or_default()
}

/// The last [`MAX_BYTES`] of the file at `path`, starting at a line.
fn read_tail(path: &Path) -> std::io::Result<Vec<u8>> {
    // A pipe or a device would block or never end; the path is checked
    // before opening, the open file again after.
    if !std::fs::metadata(path)?.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || writable_by_others(&metadata) {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let start = metadata.len().saturating_sub(MAX_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes)?;
    if start > 0 {
        // The first line is probably cut; drop it.
        let first = bytes
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(bytes.len(), |end| end + 1);
        bytes.drain(..first);
    }
    Ok(bytes)
}

/// Others could plant commands in a history file they can write, for the
/// user to pick and run.
#[cfg(unix)]
fn writable_by_others(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o022 != 0
}

#[cfg(not(unix))]
fn writable_by_others(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// Decodes a history file of `shell`, oldest command first, keeping the
/// newest [`MAX_COMMANDS`] that the editor can hold.
pub fn parse(shell: Shell, bytes: &[u8]) -> Vec<String> {
    let commands = match shell {
        Shell::Bash => bash(bytes),
        Shell::Zsh => zsh(bytes),
        Shell::Fish => fish(bytes),
        Shell::PowerShell => powershell(bytes),
    };
    let mut kept: Vec<String> = commands
        .into_iter()
        .filter_map(|command| clean(&command))
        .collect();
    let excess = kept.len().saturating_sub(MAX_COMMANDS);
    kept.drain(..excess);
    kept
}

/// Removes control characters other than line breaks and trailing line
/// breaks; `None` for commands that are empty or too long for the editor.
fn clean(command: &str) -> Option<String> {
    let cleaned: String = command
        .chars()
        .filter(|&ch| ch == '\n' || !ch.is_control())
        .collect();
    let cleaned = cleaned.trim_end_matches('\n');
    (!cleaned.trim().is_empty() && crate::editor::holds(cleaned)).then(|| cleaned.to_owned())
}

/// Splits on line breaks, accepting CRLF.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// One command per line; `#` and digits are timestamps (`HISTTIMEFORMAT`).
fn bash(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    lines(&text)
        .filter(|line| {
            !line.strip_prefix('#').is_some_and(|rest| {
                !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .map(str::to_owned)
        .collect()
}

/// zsh writes bytes 0x83 to 0x9f and 0x00 "metafied": 0x83 followed by the
/// byte XOR 0x20. Lines ending in `\` continue the command; with extended
/// history each command starts with `: <time>:<duration>;`.
fn zsh(bytes: &[u8]) -> Vec<String> {
    let mut plain = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter();
    while let Some(&byte) = iter.next() {
        if byte == 0x83 {
            if let Some(&next) = iter.next() {
                plain.push(next ^ 0x20);
            }
        } else {
            plain.push(byte);
        }
    }
    let text = String::from_utf8_lossy(&plain);
    let mut commands = Vec::new();
    let mut current: Option<String> = None;
    for line in lines(&text) {
        let (line, continues) = match line.strip_suffix('\\') {
            Some(line) => (line, true),
            None => (line, false),
        };
        let command = match current.take() {
            Some(mut command) => {
                command.push('\n');
                command.push_str(line);
                command
            }
            None => extended(line).to_owned(),
        };
        if continues {
            current = Some(command);
        } else {
            commands.push(command);
        }
    }
    commands.extend(current);
    commands
}

/// The command of a zsh extended history line `: 1700000000:0;command`.
fn extended(line: &str) -> &str {
    let Some(rest) = line.strip_prefix(": ") else {
        return line;
    };
    let Some((stamp, command)) = rest.split_once(';') else {
        return line;
    };
    let is_stamp = stamp.split_once(':').is_some_and(|(time, duration)| {
        !time.is_empty()
            && time.bytes().all(|byte| byte.is_ascii_digit())
            && duration.bytes().all(|byte| byte.is_ascii_digit())
    });
    if is_stamp { command } else { line }
}

/// fish's file is a list of `- cmd: <command>` entries with `\n` and `\\`
/// escapes, followed by indented fields.
fn fish(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    lines(&text)
        .filter_map(|line| line.strip_prefix("- cmd: "))
        .map(|command| {
            let mut decoded = String::with_capacity(command.len());
            let mut chars = command.chars();
            while let Some(ch) = chars.next() {
                match (ch, chars.clone().next()) {
                    ('\\', Some('n')) => {
                        decoded.push('\n');
                        chars.next();
                    }
                    ('\\', Some('\\')) => {
                        decoded.push('\\');
                        chars.next();
                    }
                    _ => decoded.push(ch),
                }
            }
            decoded
        })
        .collect()
}

/// PSReadLine ends every line but the last of a multi-line command with a
/// backtick.
fn powershell(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut commands = Vec::new();
    let mut current: Option<String> = None;
    for line in lines(&text) {
        let (line, continues) = match line.strip_suffix('`') {
            Some(line) => (line, true),
            None => (line, false),
        };
        let command = match current.take() {
            Some(mut command) => {
                command.push('\n');
                command.push_str(line);
                command
            }
            None => line.to_owned(),
        };
        if continues {
            current = Some(command);
        } else {
            commands.push(command);
        }
    }
    commands.extend(current);
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_skips_timestamps() {
        let file = b"ls\n#1700000000\ncd src\n#not a stamp\r\n\n";
        assert_eq!(parse(Shell::Bash, file), ["ls", "cd src", "#not a stamp"]);
    }

    #[test]
    fn zsh_reads_extended_metafied_and_continued_lines() {
        // "é" is 0xc3 0xa9; zsh does not metafy it. 0x83 0xa3 is 0x83.
        let mut file =
            b": 1700000000:0;echo one\nplain\n: 1:2;for x in a b\\\ndo echo $x\\\ndone\n".to_vec();
        file.extend_from_slice(b"echo \xc3\xa9 \x83\xa3\n");
        let commands = parse(Shell::Zsh, &file);
        assert_eq!(
            commands[..3],
            ["echo one", "plain", "for x in a b\ndo echo $x\ndone"]
        );
        // 0x83 alone is not valid UTF-8; it is replaced, not dropped.
        assert_eq!(commands[3], "echo é \u{fffd}");
        assert_eq!(parse(Shell::Zsh, b": nope;x\n"), [": nope;x"]);
    }

    #[test]
    fn fish_decodes_its_entries() {
        let file = b"- cmd: echo one\n  when: 1700000000\n- cmd: printf 'a\\nb' \\\\n\n  when: 1\n  paths:\n    - foo\n";
        assert_eq!(parse(Shell::Fish, file), ["echo one", "printf 'a\nb' \\n"]);
    }

    #[test]
    fn powershell_joins_continued_lines() {
        let file = b"Get-ChildItem\r\nif ($true) {`\r\n  'yes'`\r\n}\r\ndir\r\n";
        assert_eq!(
            parse(Shell::PowerShell, file),
            ["Get-ChildItem", "if ($true) {\n  'yes'\n}", "dir"]
        );
    }

    #[test]
    fn control_characters_and_oversized_commands_are_dropped() {
        let long = "x".repeat(5 * 1024);
        let file = format!("echo \x1b[31mred\x07\n\x1b\n{long}\nok\n");
        assert_eq!(parse(Shell::Bash, file.as_bytes()), ["echo [31mred", "ok"]);
    }

    #[test]
    fn keeps_the_newest_commands() {
        let file: String = (0..MAX_COMMANDS + 5)
            .map(|index| format!("echo {index}\n"))
            .collect();
        let commands = parse(Shell::Bash, file.as_bytes());
        assert_eq!(commands.len(), MAX_COMMANDS);
        assert_eq!(commands[0], "echo 5");
    }

    #[test]
    fn default_locations() {
        let unix = |name: &str| match name {
            "HOME" => Some(OsString::from("/home/u")),
            "ZDOTDIR" => Some(OsString::from("relative")),
            _ => None,
        };
        let path =
            |shell| path(shell, unix, false).map(|path| path.to_string_lossy().replace('\\', "/"));
        if cfg!(unix) {
            assert_eq!(path(Shell::Bash).as_deref(), Some("/home/u/.bash_history"));
            // A relative ZDOTDIR is ignored.
            assert_eq!(path(Shell::Zsh).as_deref(), Some("/home/u/.zsh_history"));
            assert_eq!(
                path(Shell::Fish).as_deref(),
                Some("/home/u/.local/share/fish/fish_history")
            );
            assert_eq!(
                path(Shell::PowerShell).as_deref(),
                Some("/home/u/.local/share/powershell/PSReadLine/ConsoleHost_history.txt")
            );
        }
        // Without any variable there is no location.
        assert_eq!(super::path(Shell::Bash, |_| None, false), None);
        if cfg!(windows) {
            let windows = |name: &str| match name {
                "USERPROFILE" => Some(OsString::from(r"C:\Users\u")),
                "APPDATA" => Some(OsString::from(r"C:\Users\u\AppData\Roaming")),
                _ => None,
            };
            assert_eq!(
                super::path(Shell::PowerShell, windows, true),
                Some(PathBuf::from(
                    r"C:\Users\u\AppData\Roaming\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt"
                ))
            );
            assert_eq!(
                super::path(Shell::Bash, windows, true),
                Some(PathBuf::from(r"C:\Users\u\.bash_history"))
            );
        }
    }

    #[test]
    fn reads_only_the_end_of_a_regular_file() {
        let dir = std::env::temp_dir().join(format!("woop-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test file system");
        let file = dir.join("history");
        let mut contents = "a".repeat(MAX_BYTES as usize);
        contents.push_str("\nnewest\n");
        std::fs::write(&file, &contents).expect("test file system");
        let bytes = read_tail(&file).expect("test file system");
        assert_eq!(bytes, b"newest\n", "the cut first line is dropped");
        // A directory is not read.
        assert!(read_tail(&dir).is_err());
        std::fs::remove_dir_all(&dir).expect("test file system");
    }

    #[cfg(unix)]
    #[test]
    fn a_file_others_can_write_is_not_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("woop-history-shared-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test file system");
        let file = dir.join("history");
        std::fs::write(&file, "planted\n").expect("test file system");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666))
            .expect("test file system");
        assert!(read_tail(&file).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
            .expect("test file system");
        assert_eq!(read_tail(&file).expect("test file system"), b"planted\n");
        std::fs::remove_dir_all(&dir).expect("test file system");
    }
}
