//! Keyboard shortcuts for Woop's actions (`[keys]` in the configuration).

use crate::ConfigError;

/// An action that can be given a keyboard shortcut, and that the command
/// palette lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    /// Open a new tab.
    NewTab,
    /// Close the focused pane.
    ClosePane,
    /// Show the next tab.
    NextTab,
    /// Show the previous tab.
    PreviousTab,
    /// Split the focused pane, with the new pane on the right.
    SplitRight,
    /// Split the focused pane, with the new pane below.
    SplitDown,
    /// Focus the pane on the left.
    FocusLeft,
    /// Focus the pane on the right.
    FocusRight,
    /// Focus the pane above.
    FocusUp,
    /// Focus the pane below.
    FocusDown,
    /// Open the command palette.
    CommandPalette,
    /// Copy the last finished command.
    CopyLastCommandLine,
    /// Copy the output of the last finished command.
    CopyLastOutput,
    /// Collapse or expand the output of the last finished command.
    ToggleLastOutput,
    /// Move the view to the previous command block.
    PreviousBlock,
    /// Move the view to the next command block.
    NextBlock,
    /// Copy the selected text (besides the platform's own copy keys).
    CopySelection,
    /// Paste the clipboard (besides the platform's own paste keys).
    Paste,
    /// Move the view back to the latest output.
    ScrollToBottom,
    /// Copy the path of the configuration file.
    CopyConfigPath,
    /// Read the configuration file again now.
    ReloadConfig,
}

impl Action {
    /// Every action.
    pub const ALL: [Self; 21] = [
        Self::NewTab,
        Self::ClosePane,
        Self::NextTab,
        Self::PreviousTab,
        Self::SplitRight,
        Self::SplitDown,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::CommandPalette,
        Self::CopyLastCommandLine,
        Self::CopyLastOutput,
        Self::ToggleLastOutput,
        Self::PreviousBlock,
        Self::NextBlock,
        Self::CopySelection,
        Self::Paste,
        Self::ScrollToBottom,
        Self::CopyConfigPath,
        Self::ReloadConfig,
    ];

    /// The name used in the configuration file.
    pub fn name(self) -> &'static str {
        match self {
            Self::NewTab => "new-tab",
            Self::ClosePane => "close-pane",
            Self::NextTab => "next-tab",
            Self::PreviousTab => "previous-tab",
            Self::SplitRight => "split-right",
            Self::SplitDown => "split-down",
            Self::FocusLeft => "focus-left",
            Self::FocusRight => "focus-right",
            Self::FocusUp => "focus-up",
            Self::FocusDown => "focus-down",
            Self::CommandPalette => "command-palette",
            Self::CopyLastCommandLine => "copy-last-command",
            Self::CopyLastOutput => "copy-last-output",
            Self::ToggleLastOutput => "toggle-last-output",
            Self::PreviousBlock => "previous-block",
            Self::NextBlock => "next-block",
            Self::CopySelection => "copy-selection",
            Self::Paste => "paste",
            Self::ScrollToBottom => "scroll-to-bottom",
            Self::CopyConfigPath => "copy-config-path",
            Self::ReloadConfig => "reload-config",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }
}

/// A key that is not a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKey {
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Tab.
    Tab,
    /// Enter.
    Enter,
    /// Space.
    Space,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// A function key, F1 to F12.
    F(u8),
}

const NAMED_KEYS: [(&str, NamedKey); 15] = [
    ("Up", NamedKey::Up),
    ("Down", NamedKey::Down),
    ("Left", NamedKey::Left),
    ("Right", NamedKey::Right),
    ("Tab", NamedKey::Tab),
    ("Enter", NamedKey::Enter),
    ("Space", NamedKey::Space),
    ("Escape", NamedKey::Escape),
    ("Backspace", NamedKey::Backspace),
    ("Delete", NamedKey::Delete),
    ("Insert", NamedKey::Insert),
    ("Home", NamedKey::Home),
    ("End", NamedKey::End),
    ("PageUp", NamedKey::PageUp),
    ("PageDown", NamedKey::PageDown),
];

/// The key of a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordKey {
    /// A character key, in lower case.
    Char(char),
    /// Any other key.
    Named(NamedKey),
}

/// A keyboard shortcut: modifiers and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    /// Control.
    pub ctrl: bool,
    /// Alt (Option on macOS).
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// Command on macOS, the Windows or Super key elsewhere.
    pub logo: bool,
    /// The key.
    pub key: ChordKey,
}

impl KeyChord {
    /// A shortcut with Control.
    pub const fn ctrl(key: ChordKey) -> Self {
        Self {
            ctrl: true,
            alt: false,
            shift: false,
            logo: false,
            key,
        }
    }

    /// A shortcut with Command (macOS) or the Windows or Super key.
    pub const fn logo(key: ChordKey) -> Self {
        Self {
            logo: true,
            ..Self::ctrl(key)
        }
        .without_ctrl()
    }

    /// A shortcut with Alt.
    pub const fn alt(key: ChordKey) -> Self {
        Self {
            alt: true,
            ..Self::ctrl(key)
        }
        .without_ctrl()
    }

    /// The same shortcut with Shift too.
    pub const fn shift(self) -> Self {
        Self {
            shift: true,
            ..self
        }
    }

    /// The same shortcut with Alt too.
    pub const fn with_alt(self) -> Self {
        Self { alt: true, ..self }
    }

    const fn without_ctrl(self) -> Self {
        Self {
            ctrl: false,
            ..self
        }
    }

    /// Writes the shortcut as people read it, such as `Ctrl+Shift+T`, or
    /// `Cmd+Option+Left` on macOS.
    pub fn label(&self, macos: bool) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.logo {
            parts.push(if macos { "Cmd" } else { "Super" }.into());
        }
        if self.alt {
            parts.push(if macos { "Option" } else { "Alt" }.into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        parts.push(match self.key {
            ChordKey::Char(ch) => ch.to_uppercase().collect(),
            ChordKey::Named(NamedKey::F(number)) => format!("F{number}"),
            ChordKey::Named(named) => NAMED_KEYS
                .iter()
                .find(|(_, key)| *key == named)
                .map_or("?", |(name, _)| name)
                .into(),
        });
        parts.join("+")
    }

    /// Parses a shortcut written as `Ctrl+Shift+T`. Modifier and key names
    /// ignore case; `Cmd`, `Super`, `Win` and `Meta` all mean the logo key,
    /// and `Option` means Alt.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, modifiers) = match parts.split_last() {
            // "Ctrl++" names the plus key.
            Some((&"", rest)) if rest.last() == Some(&"") => ("+", &rest[..rest.len() - 1]),
            Some((key, rest)) => (*key, rest),
            None => return Err("is empty".into()),
        };
        let mut chord = Self {
            ctrl: false,
            alt: false,
            shift: false,
            logo: false,
            key: parse_key(key)?,
        };
        for modifier in modifiers {
            let flag = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => &mut chord.ctrl,
                "alt" | "option" => &mut chord.alt,
                "shift" => &mut chord.shift,
                "cmd" | "command" | "super" | "win" | "meta" => &mut chord.logo,
                other => return Err(format!("has an unknown modifier `{other}`")),
            };
            if *flag {
                return Err(format!("repeats `{modifier}`"));
            }
            *flag = true;
        }
        if !(chord.ctrl || chord.alt || chord.logo) {
            return Err(
                "needs Ctrl, Alt or Cmd, so that it does not take a key used for typing".into(),
            );
        }
        // AltGr arrives as Ctrl+Alt on Windows and types characters on many
        // layouts; taking Ctrl+Alt with a character would block them.
        if chord.ctrl && chord.alt && matches!(chord.key, ChordKey::Char(_)) {
            return Err(
                "uses Ctrl+Alt with a character, which types characters (AltGr) on many keyboards"
                    .into(),
            );
        }
        Ok(chord)
    }
}

fn parse_key(text: &str) -> Result<ChordKey, String> {
    let mut chars = text.chars();
    if let (Some(ch), None) = (chars.next(), chars.next())
        && !ch.is_control()
        && !ch.is_whitespace()
    {
        return Ok(ChordKey::Char(ch.to_lowercase().next().unwrap_or(ch)));
    }
    if let Some(number) = text
        .strip_prefix(['F', 'f'])
        .and_then(|digits| digits.parse::<u8>().ok())
        .filter(|number| (1..=12).contains(number))
    {
        return Ok(ChordKey::Named(NamedKey::F(number)));
    }
    let alias = match text.to_ascii_lowercase().as_str() {
        "arrowup" => "up",
        "arrowdown" => "down",
        "arrowleft" => "left",
        "arrowright" => "right",
        "return" => "enter",
        "esc" => "escape",
        "del" => "delete",
        "plus" => return Ok(ChordKey::Char('+')),
        other => return lookup_named(other).ok_or_else(|| format!("has an unknown key `{text}`")),
    };
    lookup_named(alias).ok_or_else(|| format!("has an unknown key `{text}`"))
}

fn lookup_named(lowercase: &str) -> Option<ChordKey> {
    NAMED_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(lowercase))
        .map(|&(_, key)| ChordKey::Named(key))
}

/// Parses the `[keys]` table: each action's new shortcut, or `None` for
/// `"none"`, which removes it.
pub(crate) fn parse_bindings(
    table: std::collections::BTreeMap<String, String>,
) -> Result<Vec<(Action, Option<KeyChord>)>, ConfigError> {
    let bindings: Vec<(Action, Option<KeyChord>)> = table
        .into_iter()
        .map(|(name, value)| {
            let action = Action::from_name(&name).ok_or_else(|| {
                crate::invalid(format!("keys.{name} is not an action Woop knows"))
            })?;
            if value.trim().eq_ignore_ascii_case("none") {
                return Ok((action, None));
            }
            let chord = KeyChord::parse(&value)
                .map_err(|reason| crate::invalid(format!("keys.{name}: \"{value}\" {reason}")))?;
            Ok((action, Some(chord)))
        })
        .collect::<Result<_, ConfigError>>()?;
    // One shortcut for two actions would run only one of them.
    for (index, (action, chord)) in bindings.iter().enumerate() {
        if let Some(chord) = chord
            && let Some((other, _)) = bindings[index + 1..]
                .iter()
                .find(|(_, other)| other.as_ref() == Some(chord))
        {
            return Err(crate::invalid(format!(
                "keys.{} and keys.{} have the same shortcut",
                action.name(),
                other.name()
            )));
        }
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts() {
        let chord = KeyChord::parse("Ctrl+Shift+T").expect("valid");
        assert_eq!(chord, KeyChord::ctrl(ChordKey::Char('t')).shift());
        assert_eq!(
            KeyChord::parse("cmd+option+left"),
            Ok(KeyChord::logo(ChordKey::Named(NamedKey::Left)).with_alt())
        );
        assert_eq!(
            KeyChord::parse("Alt + F5"),
            Ok(KeyChord::alt(ChordKey::Named(NamedKey::F(5))))
        );
        assert_eq!(
            KeyChord::parse("Ctrl++"),
            Ok(KeyChord::ctrl(ChordKey::Char('+')))
        );
        assert_eq!(
            KeyChord::parse("Ctrl+Alt+Left"),
            Ok(KeyChord::ctrl(ChordKey::Named(NamedKey::Left)).with_alt())
        );
    }

    #[test]
    fn rejects_shortcuts_that_would_take_typing_keys() {
        for text in ["T", "Shift+T", "Space", "Shift+Tab"] {
            assert!(
                KeyChord::parse(text)
                    .expect_err(text)
                    .contains("needs Ctrl"),
                "{text}"
            );
        }
        // AltGr characters.
        assert!(
            KeyChord::parse("Ctrl+Alt+Q")
                .expect_err("AltGr")
                .contains("AltGr")
        );
    }

    #[test]
    fn rejects_malformed_shortcuts() {
        for text in [
            "",
            "Ctrl+",
            "Ctrl+Hyper+T",
            "Ctrl+Ctrl+T",
            "Ctrl+F13",
            "Ctrl+Nope",
        ] {
            assert!(KeyChord::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn labels_follow_the_platform() {
        let chord = KeyChord::logo(ChordKey::Named(NamedKey::Left)).with_alt();
        assert_eq!(chord.label(true), "Cmd+Option+Left");
        assert_eq!(chord.label(false), "Super+Alt+Left");
        assert_eq!(
            KeyChord::ctrl(ChordKey::Char('t')).shift().label(false),
            "Ctrl+Shift+T"
        );
    }

    #[test]
    fn every_label_parses_back() {
        for key in NAMED_KEYS
            .map(|(_, key)| key)
            .into_iter()
            .chain([NamedKey::F(12)])
        {
            let chord = KeyChord::ctrl(ChordKey::Named(key)).shift();
            assert_eq!(KeyChord::parse(&chord.label(false)), Ok(chord));
        }
    }

    #[test]
    fn action_names_are_unique_and_known() {
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(action));
        }
    }
}
