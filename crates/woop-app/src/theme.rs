//! Colours used to draw the terminal.

use iced::Color;
use woop_term::Color as CellColor;

/// A terminal colour scheme.
#[derive(Debug, Clone)]
pub struct Palette {
    /// Default text colour.
    pub foreground: Color,
    /// Default background colour.
    pub background: Color,
    /// Cursor colour.
    pub cursor: Color,
    /// Background of selected text.
    pub selection: Color,
    /// The 16 ANSI colours: 8 normal followed by 8 bright.
    pub ansi: [Color; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            foreground: rgb(0xd4d7dd),
            background: rgb(0x16181d),
            cursor: rgb(0x7aa2f7),
            selection: rgb(0x2f3d58),
            ansi: [
                rgb(0x1f2229),
                rgb(0xe06c75),
                rgb(0x98c379),
                rgb(0xe5c07b),
                rgb(0x61afef),
                rgb(0xc678dd),
                rgb(0x56b6c2),
                rgb(0xc8ccd4),
                rgb(0x5c6370),
                rgb(0xff7a85),
                rgb(0xb5e890),
                rgb(0xffd68a),
                rgb(0x82c4ff),
                rgb(0xde9bf2),
                rgb(0x7fd3de),
                rgb(0xf5f6f8),
            ],
        }
    }
}

impl Palette {
    /// Resolves a cell colour. Bold text in one of the 8 normal ANSI colours
    /// is shown in its bright variant.
    pub fn resolve(&self, color: CellColor, bold: bool) -> Color {
        match color {
            CellColor::Foreground => self.foreground,
            CellColor::Background => self.background,
            CellColor::Indexed(index) if index < 8 && bold => self.ansi[usize::from(index) + 8],
            CellColor::Indexed(index) => self.indexed(index),
            CellColor::Rgb(r, g, b) => Color::from_rgb8(r, g, b),
        }
    }

    /// Returns an entry of the 256-colour palette.
    pub fn indexed(&self, index: u8) -> Color {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let index = index - 16;
                let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
                Color::from_rgb8(level(index / 36), level((index / 6) % 6), level(index % 6))
            }
            232..=255 => {
                let gray = 8 + (index - 232) * 10;
                Color::from_rgb8(gray, gray, gray)
            }
        }
    }
}

/// Mixes a colour towards the background, used for faint text.
pub fn dim(color: Color, background: Color) -> Color {
    let mix = |a: f32, b: f32| a * 0.6 + b * 0.4;
    Color {
        r: mix(color.r, background.r),
        g: mix(color.g, background.g),
        b: mix(color.b, background.b),
        a: color.a,
    }
}

fn rgb(hex: u32) -> Color {
    let [_, r, g, b] = hex.to_be_bytes();
    Color::from_rgb8(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_default_colors() {
        let palette = Palette::default();
        assert_eq!(
            palette.resolve(CellColor::Foreground, false),
            palette.foreground
        );
        assert_eq!(
            palette.resolve(CellColor::Background, false),
            palette.background
        );
    }

    #[test]
    fn bold_brightens_normal_ansi_colors() {
        let palette = Palette::default();
        assert_eq!(
            palette.resolve(CellColor::Indexed(1), true),
            palette.ansi[9]
        );
        assert_eq!(
            palette.resolve(CellColor::Indexed(9), true),
            palette.ansi[9]
        );
    }

    #[test]
    fn computes_color_cube_and_grayscale() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(16), Color::from_rgb8(0, 0, 0));
        assert_eq!(palette.indexed(231), Color::from_rgb8(255, 255, 255));
        assert_eq!(palette.indexed(196), Color::from_rgb8(255, 0, 0));
        assert_eq!(palette.indexed(232), Color::from_rgb8(8, 8, 8));
        assert_eq!(palette.indexed(255), Color::from_rgb8(238, 238, 238));
    }

    #[test]
    fn passes_true_colors_through() {
        let palette = Palette::default();
        assert_eq!(
            palette.resolve(CellColor::Rgb(10, 20, 30), false),
            Color::from_rgb8(10, 20, 30)
        );
    }
}
