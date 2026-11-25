//! Central visual tokens and egui theme application.
//!
//! Colors, radii, spacings and type sizes live here. `gui.rs` and `ai_panel.rs`
//! consume tokens; they do not define their own chrome colors.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Margin, Shadow, Stroke, TextStyle, ThemePreference,
    Visuals,
};
use mappamente_core::theme::{Palette, parse_hex};

/// Standard panel radius.
pub const RADIUS_PANEL: u8 = 12;
/// Standard control radius.
pub const RADIUS_CONTROL: u8 = 8;
/// Pill radius cap for nodes.
pub const RADIUS_NODE_MAX: f32 = 18.0;
/// Base spacing scale.
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;
pub const SPACE_6: f32 = 32.0;
/// Type scale.
pub const FONT_TITLE: f32 = 20.0;
pub const FONT_NODE: f32 = 15.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_BUTTON: f32 = 13.0;
pub const FONT_SMALL: f32 = 11.0;
/// Default transition duration.
pub const ANIMATION_TIME: f32 = 0.18;

/// Semantic colors and shape tokens for one palette.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_muted: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub selection: Color32,
    pub shadow: Color32,
    pub grid: Color32,
    pub edge: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub success: Color32,
    /// One hue per first-level branch.
    pub branches: [Color32; 8],
}

impl Tokens {
    /// Builds tokens for a built-in map theme name.
    #[must_use]
    pub fn for_theme(theme: &str) -> Self {
        let palette = Palette::by_name(theme);
        let accent = color(palette.accent);
        let (dark, bg, surface, raised, muted, border, border_strong, text, muted_text, faint) =
            match palette.name {
                "light" => (
                    false,
                    rgb(0xf6f6f7),
                    rgb(0xffffff),
                    rgb(0xffffff),
                    rgb(0xf0f0f3),
                    rgb(0xe4e4e9),
                    rgb(0xd1d1d9),
                    rgb(0x18181b),
                    rgb(0x71717a),
                    rgb(0xa1a1aa),
                ),
                "paper" => (
                    false,
                    rgb(0xf4efe6),
                    rgb(0xfbf6ec),
                    rgb(0xfffcf6),
                    rgb(0xefe7d9),
                    rgb(0xe2d7c5),
                    rgb(0xcfc0aa),
                    rgb(0x2b241e),
                    rgb(0x7a6957),
                    rgb(0xa39078),
                ),
                "ocean" => (
                    true,
                    rgb(0x09171a),
                    rgb(0x0f2226),
                    rgb(0x143036),
                    rgb(0x18343a),
                    rgb(0x214248),
                    rgb(0x2d5961),
                    rgb(0xe6f4f1),
                    rgb(0x8fb8bb),
                    rgb(0x648f93),
                ),
                _ => (
                    true,
                    rgb(0x0d0d10),
                    rgb(0x151519),
                    rgb(0x1b1b21),
                    rgb(0x202027),
                    rgb(0x2a2a32),
                    rgb(0x3a3a45),
                    rgb(0xededf0),
                    rgb(0x9a9aa6),
                    rgb(0x6e6e7a),
                ),
            };
        let branches = palette.branch_fills().map(color);
        Self {
            dark,
            bg,
            surface,
            surface_raised: raised,
            surface_muted: muted,
            border,
            border_strong,
            text,
            text_muted: muted_text,
            text_faint: faint,
            accent,
            accent_text: readable_on(accent),
            accent_soft: mix(accent, surface, 0.84),
            selection: with_alpha(accent, 42),
            shadow: if dark {
                Color32::from_black_alpha(120)
            } else {
                Color32::from_black_alpha(26)
            },
            grid: if dark { rgb(0x23232b) } else { rgb(0xd9d9e0) },
            edge: if dark { rgb(0x4a4a58) } else { rgb(0xb6b6c1) },
            error: rgb(0xd33a3a),
            warning: rgb(0xd97706),
            success: rgb(0x2e9e6b),
            branches,
        }
    }

    /// Fill for a node. Explicit node colors win; root uses the accent.
    #[must_use]
    pub fn node_fill(
        &self,
        branch: Option<usize>,
        depth: u32,
        explicit: Option<Color32>,
    ) -> Color32 {
        if let Some(explicit) = explicit {
            return explicit;
        }
        if depth == 0 {
            return self.accent;
        }
        let Some(branch) = branch else {
            return self.surface_muted;
        };
        let base = self.branches[branch % self.branches.len()];
        if depth <= 1 {
            return base;
        }
        let factor = ((depth - 1) as f32 * 0.18).min(if self.dark { 0.55 } else { 0.68 });
        mix(base, self.surface, factor)
    }

    /// Best readable text color on a node fill.
    #[must_use]
    pub fn node_text(&self, fill: Color32, depth: u32) -> Color32 {
        if depth == 0 && fill == self.accent {
            return self.accent_text;
        }
        readable_on(fill)
    }

    /// Branch-colored connector with a depth-dependent alpha.
    #[must_use]
    pub fn edge_color(&self, branch: Option<usize>, depth: u32) -> Color32 {
        let base = self
            .branches
            .get(branch.unwrap_or(0) % self.branches.len())
            .copied()
            .unwrap_or(self.edge);
        let alpha = 220u32.saturating_sub(depth * 16).clamp(110, 220);
        with_alpha(base, u8::try_from(alpha).unwrap_or(180))
    }

    /// Hover/selection halo color.
    #[must_use]
    pub fn halo(&self) -> Color32 {
        with_alpha(self.accent, 44)
    }

    /// Soft shadow for the floating toolbar.
    #[must_use]
    pub fn floating_shadow(&self) -> Shadow {
        Shadow {
            offset: [0, 6],
            blur: 24,
            spread: 0,
            color: self.shadow,
        }
    }

    /// Smaller shadow for the status pill.
    #[must_use]
    pub fn status_shadow(&self) -> Shadow {
        Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: self.shadow,
        }
    }
}

/// Applies tokens and typography to an egui context.
pub fn apply(context: &egui::Context, tokens: &Tokens, animations: bool) {
    context.set_theme(if tokens.dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });

    context.all_styles_mut(|style| {
        style.animation_time = if animations { ANIMATION_TIME } else { 0.0 };
        style.spacing.item_spacing = egui::vec2(SPACE_2, SPACE_2);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size = egui::vec2(36.0, 32.0);
        style.spacing.window_margin = Margin::same(12);
        style.spacing.menu_margin = Margin::same(8);
        style.spacing.indent = 18.0;
        style.text_styles = BTreeMap::from([
            (TextStyle::Heading, FontId::proportional(FONT_TITLE)),
            (TextStyle::Body, FontId::proportional(FONT_BODY)),
            (TextStyle::Button, FontId::proportional(FONT_BUTTON)),
            (TextStyle::Small, FontId::proportional(FONT_SMALL)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]);

        let mut visuals = if tokens.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.dark_mode = tokens.dark;
        visuals.panel_fill = tokens.bg;
        visuals.window_fill = tokens.surface;
        visuals.window_stroke = Stroke::new(1.0, tokens.border);
        visuals.window_corner_radius = CornerRadius::same(RADIUS_PANEL);
        visuals.menu_corner_radius = CornerRadius::same(RADIUS_CONTROL);
        visuals.window_shadow = Shadow {
            offset: [0, 10],
            blur: 30,
            spread: 0,
            color: tokens.shadow,
        };
        visuals.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: tokens.shadow,
        };
        visuals.extreme_bg_color = tokens.surface_muted;
        visuals.faint_bg_color = tokens.surface_muted;
        visuals.text_edit_bg_color = Some(tokens.surface_muted);
        visuals.code_bg_color = tokens.surface_muted;
        visuals.weak_text_color = Some(tokens.text_muted);
        visuals.hyperlink_color = tokens.accent;
        visuals.warn_fg_color = tokens.warning;
        visuals.error_fg_color = tokens.error;
        visuals.selection.bg_fill = tokens.selection;
        visuals.selection.stroke = Stroke::new(1.0, tokens.accent);

        visuals.widgets.noninteractive.bg_fill = tokens.surface;
        visuals.widgets.noninteractive.weak_bg_fill = tokens.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, tokens.text);

        visuals.widgets.inactive.bg_fill = tokens.surface;
        visuals.widgets.inactive.weak_bg_fill = tokens.surface_muted;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, tokens.text);

        visuals.widgets.hovered.bg_fill = tokens.surface_raised;
        visuals.widgets.hovered.weak_bg_fill = tokens.surface_raised;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.accent_soft);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, tokens.text);

        visuals.widgets.active.bg_fill = tokens.surface_raised;
        visuals.widgets.active.weak_bg_fill = tokens.surface_raised;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, tokens.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, tokens.accent);
        visuals.widgets.open = visuals.widgets.active;

        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::same(RADIUS_CONTROL);
        }
        style.visuals = visuals;
    });
}

/// Reads a `#RRGGBB`/`#RRGGBBAA` string into an egui color.
#[must_use]
pub fn color(value: &str) -> Color32 {
    parse_hex(value)
        .map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
        .unwrap_or(Color32::TRANSPARENT)
}

/// Applies an alpha channel to a color.
#[must_use]
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Multiplies a color's existing alpha by `factor` (0..=1).
#[must_use]
pub fn fade(color: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    let alpha = (f32::from(color.a()) * factor).round().clamp(0.0, 255.0) as u8;
    with_alpha(color, alpha)
}

/// Linear sRGB blend; `amount` 0 keeps `a`, 1 returns `b`.
#[must_use]
pub fn mix(a: Color32, b: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |left: u8, right: u8| {
        let value = f32::from(left) + (f32::from(right) - f32::from(left)) * amount;
        value.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
}

/// Picks white or near-black, whichever has better contrast on `background`.
#[must_use]
pub fn readable_on(background: Color32) -> Color32 {
    let luminance = relative_luminance(background);
    let white = 1.05 / (luminance + 0.05);
    let black = (luminance + 0.05) / 0.05;
    if white >= black {
        Color32::WHITE
    } else {
        rgb(0x111827)
    }
}

fn relative_luminance(color: Color32) -> f32 {
    let channel = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

const fn rgb(value: u32) -> Color32 {
    Color32::from_rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_text_uses_contrast() {
        assert_eq!(readable_on(Color32::BLACK), Color32::WHITE);
        assert_eq!(readable_on(Color32::WHITE), rgb(0x111827));
    }

    #[test]
    fn branch_fills_are_distinct_and_theme_aware() {
        let light = Tokens::for_theme("light");
        let dark = Tokens::for_theme("dark");
        assert!(!light.dark);
        assert!(dark.dark);
        let mut seen = std::collections::HashSet::new();
        for index in 0..8 {
            assert!(seen.insert(light.branches[index].to_array()));
        }
        assert_ne!(
            light.node_fill(Some(0), 1, None),
            light.node_fill(Some(1), 1, None)
        );
        assert_eq!(
            light.node_fill(Some(0), 1, Some(Color32::RED)),
            Color32::RED
        );
    }

    #[test]
    fn depth_tints_move_toward_the_surface() {
        let tokens = Tokens::for_theme("light");
        let depth_one = tokens.node_fill(Some(0), 1, None);
        let depth_three = tokens.node_fill(Some(0), 3, None);
        assert_ne!(depth_one, depth_three);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.0), Color32::BLACK);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
    }
}
