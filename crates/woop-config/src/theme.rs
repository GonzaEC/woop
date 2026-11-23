//! Colour themes: the built-in ones and the user's overrides.

use serde::Deserialize;

use crate::ConfigError;

/// A colour as red, green and blue components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    const fn hex(value: u32) -> Self {
        let [_, r, g, b] = value.to_be_bytes();
        Self(r, g, b)
    }

    /// Parses `#rrggbb`.
    fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(digits, 16).ok().map(Self::hex)
    }
}

/// Every colour Woop draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colors {
    /// Default text.
    pub foreground: Rgb,
    /// Default background.
    pub background: Rgb,
    /// The cursor, the focused pane's outline and the active editor.
    pub cursor: Rgb,
    /// Selected text.
    pub selection: Rgb,
    /// Lines between command blocks.
    pub separator: Rgb,
    /// Commands that succeeded.
    pub success: Rgb,
    /// Commands that failed, and warnings.
    pub failure: Rgb,
    /// The 16 ANSI colours: 8 normal followed by 8 bright.
    pub ansi: [Rgb; 16],
}

/// Names of the built-in themes, the first being the default.
pub const THEMES: [&str; 2] = ["dark", "light"];

impl Colors {
    /// The default dark theme.
    pub const fn dark() -> Self {
        Self {
            foreground: Rgb::hex(0xd4d7dd),
            background: Rgb::hex(0x16181d),
            cursor: Rgb::hex(0x7aa2f7),
            selection: Rgb::hex(0x2f3d58),
            separator: Rgb::hex(0x2c313a),
            success: Rgb::hex(0x98c379),
            failure: Rgb::hex(0xe06c75),
            ansi: [
                Rgb::hex(0x1f2229),
                Rgb::hex(0xe06c75),
                Rgb::hex(0x98c379),
                Rgb::hex(0xe5c07b),
                Rgb::hex(0x61afef),
                Rgb::hex(0xc678dd),
                Rgb::hex(0x56b6c2),
                Rgb::hex(0xc8ccd4),
                Rgb::hex(0x5c6370),
                Rgb::hex(0xff7a85),
                Rgb::hex(0xb5e890),
                Rgb::hex(0xffd68a),
                Rgb::hex(0x82c4ff),
                Rgb::hex(0xde9bf2),
                Rgb::hex(0x7fd3de),
                Rgb::hex(0xf5f6f8),
            ],
        }
    }

    /// A light theme.
    pub const fn light() -> Self {
        Self {
            foreground: Rgb::hex(0x24292f),
            background: Rgb::hex(0xfafafa),
            cursor: Rgb::hex(0x2f6feb),
            selection: Rgb::hex(0xc8d9f5),
            separator: Rgb::hex(0xd8dce1),
            success: Rgb::hex(0x2e8b3e),
            failure: Rgb::hex(0xcf222e),
            ansi: [
                Rgb::hex(0x24292f),
                Rgb::hex(0xcf222e),
                Rgb::hex(0x116329),
                Rgb::hex(0x7d4e00),
                Rgb::hex(0x0550ae),
                Rgb::hex(0x8250df),
                Rgb::hex(0x1b7c83),
                Rgb::hex(0x6e7781),
                Rgb::hex(0x57606a),
                Rgb::hex(0xa40e26),
                Rgb::hex(0x1a7f37),
                Rgb::hex(0x633c01),
                Rgb::hex(0x0969da),
                Rgb::hex(0x6639ba),
                Rgb::hex(0x3192aa),
                Rgb::hex(0x8c959f),
            ],
        }
    }

    fn named(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }
}

/// The `[colors]` table as written in the file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawColors {
    theme: Option<String>,
    foreground: Option<String>,
    background: Option<String>,
    cursor: Option<String>,
    selection: Option<String>,
    ansi: Option<Vec<String>>,
}

impl RawColors {
    /// The named theme (the default one if none), with the overrides.
    pub(crate) fn resolve(self) -> Result<Colors, ConfigError> {
        let mut colors = match self.theme.as_deref() {
            None => Colors::dark(),
            Some(name) => Colors::named(name).ok_or_else(|| {
                crate::invalid(format!(
                    "colors.theme must be one of: {}",
                    THEMES.join(", ")
                ))
            })?,
        };
        let color = |name: &str, value: Option<String>, current: Rgb| match value {
            None => Ok(current),
            Some(text) => Rgb::parse(&text)
                .ok_or_else(|| crate::invalid(format!("colors.{name} must be written as #rrggbb"))),
        };
        colors.foreground = color("foreground", self.foreground, colors.foreground)?;
        colors.background = color("background", self.background, colors.background)?;
        colors.cursor = color("cursor", self.cursor, colors.cursor)?;
        colors.selection = color("selection", self.selection, colors.selection)?;
        if let Some(ansi) = self.ansi {
            if ansi.len() != 16 {
                return Err(crate::invalid(
                    "colors.ansi must list 16 colours: 8 normal, then 8 bright".into(),
                ));
            }
            for (slot, text) in colors.ansi.iter_mut().zip(ansi) {
                *slot = color("ansi", Some(text), *slot)?;
            }
        }
        Ok(colors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(Rgb::parse("#0a1B2c"), Some(Rgb(0x0a, 0x1b, 0x2c)));
        for bad in ["0a1b2c", "#0a1b2", "#0a1b2c0", "#0a1b2g", "#+a1b2c", ""] {
            assert_eq!(Rgb::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_theme_is_listed() {
        for name in THEMES {
            assert!(Colors::named(name).is_some(), "{name}");
        }
    }
}
