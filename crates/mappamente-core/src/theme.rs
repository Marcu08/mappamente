//! Built-in color palettes shared by the GUI and SVG export.
//!
//! Colors are stored as hex strings so this crate stays free of GUI types.
//! Use [`parse_hex`] to convert them to bytes in the frontend.

use crate::model::is_valid_color;

/// A complete color set for a map.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub background: &'static str,
    pub toolbar: &'static str,
    pub grid: &'static str,
    pub text: &'static str,
    pub muted_text: &'static str,
    pub edge: &'static str,
    /// Fill of the root node and selected/accent elements.
    pub accent: &'static str,
    /// Text color on top of [`Palette::accent`].
    pub accent_text: &'static str,
    /// Node fills by depth, starting at depth 1. The last entry is reused.
    pub depth_fills: [&'static str; 4],
}

impl Palette {
    /// Returns a built-in palette by name, falling back to `dark`.
    #[must_use]
    pub fn by_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("light") {
            LIGHT
        } else if name.eq_ignore_ascii_case("paper") {
            PAPER
        } else if name.eq_ignore_ascii_case("ocean") {
            OCEAN
        } else {
            DARK
        }
    }

    /// Fill used for a node at `depth` (root is 0).
    ///
    /// The root always uses the accent color; deeper levels become
    /// progressively more subdued.
    #[must_use]
    pub fn node_fill(&self, depth: u32) -> &'static str {
        if depth == 0 {
            self.accent
        } else {
            let index = usize::try_from(depth.saturating_sub(1)).unwrap_or(usize::MAX);
            self.depth_fills[index.min(self.depth_fills.len() - 1)]
        }
    }

    /// All names accepted by [`Palette::by_name`].
    pub const NAMES: [&'static str; 4] = ["dark", "light", "paper", "ocean"];
}

pub const DARK: Palette = Palette {
    name: "dark",
    background: "#0e1014",
    toolbar: "#151922",
    grid: "#202632",
    text: "#e8ecf4",
    muted_text: "#8b96aa",
    edge: "#3b465c",
    accent: "#4f8cff",
    accent_text: "#08111f",
    depth_fills: ["#182236", "#1c2942", "#213250", "#283c60"],
};

pub const LIGHT: Palette = Palette {
    name: "light",
    background: "#f7f8fb",
    toolbar: "#ffffff",
    grid: "#e4e8f1",
    text: "#1b2333",
    muted_text: "#6b7589",
    edge: "#c3cbda",
    accent: "#2f6fed",
    accent_text: "#ffffff",
    depth_fills: ["#eaf1ff", "#dce8ff", "#cfe0ff", "#c2d7ff"],
};

pub const PAPER: Palette = Palette {
    name: "paper",
    background: "#f7f2e8",
    toolbar: "#fffaf1",
    grid: "#e7ddcc",
    text: "#3a2f28",
    muted_text: "#8a7864",
    edge: "#c8b7a0",
    accent: "#b4532a",
    accent_text: "#ffffff",
    depth_fills: ["#f1e6d5", "#ecdfc9", "#e6d7bd", "#dfceb0"],
};

pub const OCEAN: Palette = Palette {
    name: "ocean",
    background: "#071a23",
    toolbar: "#0b2430",
    grid: "#12323f",
    text: "#dff6f8",
    muted_text: "#7ba6af",
    edge: "#2d5a6a",
    accent: "#2dd4bf",
    accent_text: "#04201c",
    depth_fills: ["#0d2a36", "#113542", "#15404f", "#194b5c"],
};

/// Parses `#RRGGBB` or `#RRGGBBAA` into RGBA bytes.
#[must_use]
pub fn parse_hex(value: &str) -> Option<[u8; 4]> {
    if !is_valid_color(value) {
        return None;
    }
    let byte = |range: std::ops::Range<usize>| u8::from_str_radix(&value[range], 16).ok();
    let r = byte(1..3)?;
    let g = byte(3..5)?;
    let b = byte(5..7)?;
    let a = if value.len() == 9 { byte(7..9)? } else { 255 };
    Some([r, g, b, a])
}

/// Whether a color is light enough to need dark text on top of it.
#[must_use]
pub fn is_light_color(value: &str) -> bool {
    let Some([r, g, b, _]) = parse_hex(value) else {
        return false;
    };
    let luminance = (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0;
    luminance > 0.6
}

/// Best readable text color (`#111827` or `#ffffff`) for a background.
#[must_use]
pub fn readable_text(value: &str) -> &'static str {
    if is_light_color(value) {
        "#111827"
    } else {
        "#ffffff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_are_complete_and_parseable() {
        for name in Palette::NAMES {
            let palette = Palette::by_name(name);
            assert_eq!(palette.name, name);
            for color in [
                palette.background,
                palette.toolbar,
                palette.grid,
                palette.text,
                palette.muted_text,
                palette.edge,
                palette.accent,
                palette.accent_text,
                palette.depth_fills[0],
                palette.depth_fills[3],
            ] {
                assert!(parse_hex(color).is_some(), "{name}: bad color {color}");
            }
        }
    }

    #[test]
    fn unknown_theme_falls_back_to_dark() {
        assert_eq!(Palette::by_name("nope").name, "dark");
        assert_eq!(Palette::by_name("LIGHT").name, "light");
    }

    #[test]
    fn depth_fill_clamps_to_the_last_entry() {
        let palette = Palette::by_name("dark");
        assert_eq!(palette.node_fill(0), palette.accent);
        assert_eq!(palette.node_fill(1), palette.depth_fills[0]);
        assert_eq!(palette.node_fill(4), palette.depth_fills[3]);
        assert_eq!(palette.node_fill(99), palette.depth_fills[3]);
    }

    #[test]
    fn hex_parsing_and_contrast() {
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0, 255]));
        assert_eq!(parse_hex("#00000080"), Some([0, 0, 0, 128]));
        assert_eq!(parse_hex("nope"), None);
        assert!(!is_light_color("#4f8cff"));
        assert!(is_light_color("#f7f8fb"));
        assert_eq!(readable_text("#f7f8fb"), "#111827");
        assert_eq!(readable_text("#0e1014"), "#ffffff");
    }
}
