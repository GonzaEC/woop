//! Woop's configuration file (ADR 0020).
//!
//! The user's `config.toml` is read, validated and turned into [`Config`],
//! whose types are Woop's own: the interface converts them into fonts,
//! colours and the shell command. [`watch`] reports changes to the file so
//! they apply without restarting.

mod theme;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

pub use theme::{Colors, Rgb, THEMES};

/// Largest configuration file read, in bytes.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Longest text accepted for a name, the program or an argument.
const MAX_TEXT_CHARS: usize = 4_096;

/// Most arguments accepted for the shell.
const MAX_ARGS: usize = 64;

/// Font sizes accepted, in points.
const FONT_SIZES: std::ops::RangeInclusive<f32> = 6.0..=72.0;

/// Font size used when the file sets none.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// Longest error message kept, so a strange file cannot produce a huge one.
const MAX_ERROR_CHARS: usize = 600;

/// The settings Woop runs with.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The font of the grid and the command editor.
    pub font: Font,
    /// The shell started in new panes; `None` uses the user's default.
    pub shell: Option<Shell>,
    /// The colours, from a theme and the user's overrides.
    pub colors: Colors,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: Font::default(),
            shell: None,
            colors: Colors::dark(),
        }
    }
}

/// Font settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    /// The font family; `None` uses the platform's monospace font.
    pub family: Option<String>,
    /// The size in points.
    pub size: f32,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: None,
            size: DEFAULT_FONT_SIZE,
        }
    }
}

/// The shell program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// The program, by name (looked up in `PATH`) or by path.
    pub program: String,
    /// Arguments passed to it.
    pub args: Vec<String>,
}

/// Why the configuration could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("cannot read {path}: {message}")]
    Read {
        /// The file.
        path: String,
        /// The reason.
        message: String,
    },
    /// The file is larger than Woop reads.
    #[error("{path} is larger than {MAX_FILE_BYTES} bytes")]
    TooLarge {
        /// The file.
        path: String,
    },
    /// Other users can change the file, which chooses the program Woop runs.
    #[error("{path} can be changed by other users; fix its permissions to use it")]
    NotPrivate {
        /// The file or directory.
        path: String,
    },
    /// The file is not valid TOML, or does not match what Woop expects.
    #[error("{0}")]
    Invalid(String),
}

/// The configuration file's path, if the user's configuration directory is
/// known: `%APPDATA%\woop\config.toml` on Windows, otherwise
/// `$XDG_CONFIG_HOME/woop/config.toml` or `~/.config/woop/config.toml`.
pub fn path() -> Option<PathBuf> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
    };
    let dir = if cfg!(windows) {
        absolute("APPDATA")?
    } else {
        absolute("XDG_CONFIG_HOME").or_else(|| Some(absolute("HOME")?.join(".config")))?
    };
    Some(dir.join("woop").join("config.toml"))
}

/// Reads the configuration at `path`. A missing file gives `Ok(None)`.
pub fn load(path: &Path) -> Result<Option<Config>, ConfigError> {
    let shown = path.display().to_string();
    let read_error = |error: std::io::Error| ConfigError::Read {
        path: shown.clone(),
        message: error.to_string(),
    };
    // Everything is checked on the open file, not on the path again, so the
    // file cannot be swapped between the checks and the read.
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read_error(error)),
    };
    let metadata = file.metadata().map_err(read_error)?;
    // Devices and pipes could be read forever or block the reader.
    if !metadata.is_file() {
        return Err(ConfigError::Invalid(format!(
            "{shown} is not a regular file"
        )));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(ConfigError::TooLarge { path: shown });
    }
    check_private(path, &metadata)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(file, MAX_FILE_BYTES + 1),
        &mut bytes,
    )
    .map_err(read_error)?;
    // The file may have grown since it was measured.
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ConfigError::TooLarge { path: shown });
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| ConfigError::Invalid(format!("{shown} is not valid UTF-8 text")))?;
    parse(&text).map(Some)
}

/// On Unix, refuses a file that other users can write, or that sits in a
/// directory they can write: it chooses the program Woop runs.
#[cfg(unix)]
fn check_private(path: &Path, metadata: &std::fs::Metadata) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    let writable_by_others = |mode: u32| mode & 0o022 != 0;
    if writable_by_others(metadata.permissions().mode()) {
        return Err(ConfigError::NotPrivate {
            path: path.display().to_string(),
        });
    }
    if let Some(dir) = path.parent()
        && let Ok(dir_metadata) = std::fs::metadata(dir)
        && writable_by_others(dir_metadata.permissions().mode())
    {
        return Err(ConfigError::NotPrivate {
            path: dir.display().to_string(),
        });
    }
    Ok(())
}

/// Elsewhere, files in the user's profile are private to the user.
#[cfg(not(unix))]
fn check_private(_path: &Path, _metadata: &std::fs::Metadata) -> Result<(), ConfigError> {
    Ok(())
}

/// Parses and validates configuration text.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(text).map_err(|error| invalid(error.to_string()))?;
    let mut config = Config::default();

    if let Some(font) = raw.font {
        if let Some(family) = font.family {
            config.font.family = Some(text_setting("font.family", family)?);
        }
        if let Some(size) = font.size {
            let size = size.value();
            if !FONT_SIZES.contains(&size) {
                return Err(invalid(format!(
                    "font.size must be between {} and {}, not {size}",
                    FONT_SIZES.start(),
                    FONT_SIZES.end()
                )));
            }
            config.font.size = size;
        }
    }

    if let Some(shell) = raw.shell {
        let program = text_setting("shell.program", shell.program)?;
        let args = shell.args.unwrap_or_default();
        if args.len() > MAX_ARGS {
            return Err(invalid(format!(
                "shell.args has more than {MAX_ARGS} arguments"
            )));
        }
        let args = args
            .into_iter()
            .map(|arg| bounded("shell.args", arg))
            .collect::<Result<_, _>>()?;
        config.shell = Some(Shell { program, args });
    }

    if let Some(colors) = raw.colors {
        config.colors = colors.resolve()?;
    }
    Ok(config)
}

/// Calls `on_change` from a background thread whenever the file at `path`
/// is created, changed or removed, checking its modification time and size
/// every `interval`. The thread runs until the program exits.
pub fn watch(path: PathBuf, interval: Duration, on_change: impl Fn() + Send + 'static) {
    let stamp = |path: &Path| -> Option<(SystemTime, u64)> {
        let metadata = std::fs::metadata(path).ok()?;
        Some((metadata.modified().ok()?, metadata.len()))
    };
    let spawned = std::thread::Builder::new()
        .name("woop-config-watch".into())
        .spawn(move || {
            let mut last = stamp(&path);
            loop {
                std::thread::sleep(interval);
                let now = stamp(&path);
                if now != last {
                    last = now;
                    on_change();
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("woop: cannot watch the configuration file: {error}");
    }
}

/// Most lines of an error message shown.
const MAX_ERROR_LINES: usize = 8;

fn invalid(message: String) -> ConfigError {
    // The message can quote the file: control and direction-changing
    // characters are replaced, and its length bounded, so it shows as text.
    let message: String = message
        .trim()
        .lines()
        .take(MAX_ERROR_LINES)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .map(|ch| {
            if (ch.is_control() && ch != '\n') || is_direction_override(ch) {
                '\u{FFFD}'
            } else {
                ch
            }
        })
        .take(MAX_ERROR_CHARS)
        .collect();
    let mut message = message;
    if message.is_empty() {
        message.push_str("invalid configuration");
    }
    ConfigError::Invalid(message)
}

/// A non-empty setting of bounded length.
fn text_setting(name: &str, value: String) -> Result<String, ConfigError> {
    let value = bounded(name, value)?;
    if value.trim().is_empty() {
        return Err(invalid(format!("{name} must not be empty")));
    }
    Ok(value)
}

fn bounded(name: &str, value: String) -> Result<String, ConfigError> {
    if value.chars().count() > MAX_TEXT_CHARS {
        return Err(invalid(format!(
            "{name} is longer than {MAX_TEXT_CHARS} characters"
        )));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || is_direction_override(ch))
    {
        return Err(invalid(format!("{name} contains a control character")));
    }
    Ok(value)
}

/// Characters that change the direction of the text around them.
fn is_direction_override(ch: char) -> bool {
    matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    font: Option<RawFont>,
    shell: Option<RawShell>,
    colors: Option<theme::RawColors>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFont {
    family: Option<String>,
    size: Option<Number>,
}

/// A number written with or without a fraction (`14` or `13.5`).
#[derive(Deserialize)]
#[serde(untagged)]
enum Number {
    Integer(i64),
    Float(f64),
}

impl Number {
    fn value(&self) -> f32 {
        match *self {
            Self::Integer(value) => value as f32,
            Self::Float(value) => value as f32,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawShell {
    program: String,
    args: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(parse("").expect("valid"), Config::default());
    }

    #[test]
    fn reads_every_setting() {
        let config = parse(
            r##"
            [font]
            family = "Cascadia Mono"
            size = 13.5

            [shell]
            program = "pwsh.exe"
            args = ["-NoLogo"]

            [colors]
            theme = "light"
            background = "#101010"
            "##,
        )
        .expect("valid");
        assert_eq!(config.font.family.as_deref(), Some("Cascadia Mono"));
        assert_eq!(config.font.size, 13.5);
        assert_eq!(
            config.shell,
            Some(Shell {
                program: "pwsh.exe".into(),
                args: vec!["-NoLogo".into()]
            })
        );
        assert_eq!(config.colors.background, Rgb(0x10, 0x10, 0x10));
        assert_eq!(config.colors.foreground, Colors::light().foreground);
    }

    #[test]
    fn whole_font_sizes_are_accepted() {
        assert_eq!(parse("[font]\nsize = 16").expect("valid").font.size, 16.0);
    }

    #[test]
    fn rejects_typos_and_bad_values_with_a_reason() {
        let error = |text: &str| parse(text).expect_err(text).to_string();
        assert!(error("[font]\nsise = 12").contains("sise"));
        assert!(error("[fonts]\nsize = 12").contains("fonts"));
        assert!(error("[font]\nsize = 200").contains("between 6 and 72"));
        assert!(error("[font]\nsize = \"big\"").contains("size"));
        assert!(error("[font]\nfamily = \"\"").contains("must not be empty"));
        assert!(error("[shell]\nargs = []").contains("program"));
        assert!(error("[colors]\nbackground = \"red\"").contains("#rrggbb"));
        assert!(error("[colors]\ntheme = \"neon\"").contains("dark"));
        assert!(error("[colors]\nansi = [\"#000000\"]").contains("16"));
        assert!(error("not toml at all").contains("line 1"));
    }

    #[test]
    fn bounds_what_it_accepts() {
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        assert!(parse(&format!("[shell]\nprogram = \"{long}\"")).is_err());
        let args = vec!["\"a\""; MAX_ARGS + 1].join(",");
        assert!(parse(&format!("[shell]\nprogram = \"sh\"\nargs = [{args}]")).is_err());
        assert!(parse("[shell]\nprogram = \"sh\\u0000\"").is_err());
        // Deep nesting is refused by the parser instead of overflowing the
        // stack.
        let deep = format!("x = {}{}", "[".repeat(10_000), "]".repeat(10_000));
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn error_messages_show_as_plain_text() {
        let ConfigError::Invalid(message) =
            parse("\"\\u001b[2J\\u202e\\n\\n\\n\\n\\n\\n\\n\\n\\n\\n\" = 1")
                .expect_err("unknown key")
        else {
            panic!("not an invalid-file error");
        };
        assert!(
            !message.contains('\u{1b}') && !message.contains('\u{202e}'),
            "{message:?}"
        );
        assert!(message.lines().count() <= MAX_ERROR_LINES, "{message:?}");
        assert!(parse("[font]\nfamily = \"Mono\\u0007\"").is_err());
        assert!(parse("[shell]\nprogram = \"sh\"\nargs = [\"a\\nb\"]").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_files_that_are_not_regular() {
        let dir = scratch("special");
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink("/dev/zero", &link).expect("symlink");
        assert!(load(&link).is_err(), "a device is not read");
    }

    #[test]
    fn error_messages_are_bounded() {
        let text = format!("[font]\nfamily = \"{}", "y".repeat(50_000));
        let ConfigError::Invalid(message) = parse(&text).expect_err("unterminated") else {
            panic!("not an invalid-file error");
        };
        assert!(message.chars().count() <= MAX_ERROR_CHARS);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("woop-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = scratch("missing");
        assert_eq!(load(&dir.join("config.toml")), Ok(None));
    }

    #[test]
    fn loads_a_file_and_refuses_large_ones() {
        let dir = scratch("load");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[font]\nsize = 20\n").expect("write");
        assert_eq!(
            load(&path).expect("valid").expect("present").font.size,
            20.0
        );

        std::fs::write(&path, "#".repeat(MAX_FILE_BYTES as usize + 1)).expect("write");
        assert!(matches!(load(&path), Err(ConfigError::TooLarge { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_files_others_can_change() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        let path = dir.join("config.toml");
        std::fs::write(&path, "").expect("write");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).expect("chmod");
        assert!(matches!(load(&path), Err(ConfigError::NotPrivate { .. })));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(load(&path).is_ok());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).expect("chmod");
        assert!(matches!(load(&path), Err(ConfigError::NotPrivate { .. })));
    }

    #[test]
    fn watching_reports_changes() {
        let dir = scratch("watch");
        let path = dir.join("config.toml");
        let (sender, changes) = std::sync::mpsc::channel();
        watch(path.clone(), Duration::from_millis(20), move || {
            let _ = sender.send(());
        });
        std::fs::write(&path, "[font]\nsize = 12\n").expect("write");
        assert!(
            changes.recv_timeout(Duration::from_secs(10)).is_ok(),
            "the new file was not noticed"
        );
    }
}
