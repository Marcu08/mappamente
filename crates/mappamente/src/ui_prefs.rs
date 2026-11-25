//! Small, user-local UI preferences.
//!
//! This file only stores view preferences (theme, grid, motion). It never
//! stores map data or API keys and lives in the platform config directory, not
//! next to `.mappa` files.

use std::fs;
use std::path::PathBuf;

use mappamente_core::theme::Palette;

/// View preferences persisted between sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiPrefs {
    /// Manual theme choice; `None` follows the OS on the next new map.
    pub theme: Option<String>,
    /// Whether the dot grid is visible.
    pub grid: bool,
    /// Whether short transitions are enabled.
    pub animations: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: None,
            grid: true,
            animations: true,
        }
    }
}

impl UiPrefs {
    /// Loads preferences, falling back to defaults on any error.
    #[must_use]
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut prefs = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim().trim_start_matches('\u{feff}') {
                "theme" => {
                    let valid = Palette::NAMES
                        .iter()
                        .find(|name| name.eq_ignore_ascii_case(value));
                    prefs.theme = valid.map(|name| (*name).to_owned());
                }
                "grid" => prefs.grid = value != "off",
                "animations" => prefs.animations = value != "off",
                _ => {}
            }
        }
        prefs
    }

    /// Saves preferences. Filesystem errors are intentionally ignored: they
    /// must never block the editor.
    pub fn save(&self) {
        let Some(path) = config_path() else {
            return;
        };
        if let Some(parent) = path.parent()
            && fs::create_dir_all(parent).is_err()
        {
            return;
        }
        let theme = self.theme.as_deref().unwrap_or("system");
        let text = format!(
            "theme={theme}\ngrid={}\nanimations={}\n",
            if self.grid { "on" } else { "off" },
            if self.animations { "on" } else { "off" }
        );
        let _ = fs::write(path, text);
    }

    /// Resolves the theme for a new or welcome map.
    #[must_use]
    pub fn theme_or(&self, system_theme: Option<&str>) -> String {
        self.theme
            .clone()
            .or_else(|| system_theme.map(str::to_owned))
            .unwrap_or_else(|| "dark".to_owned())
    }
}

fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(|base| PathBuf::from(base).join("mappamente").join("ui.conf"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|base| {
            PathBuf::from(base)
                .join("Library")
                .join("Application Support")
                .join("mappamente")
                .join("ui.conf")
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|base| PathBuf::from(base).join(".config")))
            .map(|base| base.join("mappamente").join("ui.conf"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_theme_resolution() {
        let prefs = UiPrefs::default();
        assert!(prefs.grid);
        assert!(prefs.animations);
        assert_eq!(prefs.theme_or(Some("light")), "light");
        assert_eq!(prefs.theme_or(None), "dark");

        let manual = UiPrefs {
            theme: Some("paper".to_owned()),
            ..UiPrefs::default()
        };
        assert_eq!(manual.theme_or(Some("dark")), "paper");
    }
}
