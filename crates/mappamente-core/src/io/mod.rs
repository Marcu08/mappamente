//! Reading and writing map files.

pub mod json;
pub mod markdown;

use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::model::MindMap;

/// Loads a map, choosing the format from the file extension.
///
/// Supported inputs: `.mappa`/`.json` (native) and `.md`/`.markdown` (outline).
pub fn load_path(path: &Path) -> Result<MindMap> {
    match extension(path).as_deref() {
        Some("mappa" | "json") => json::load(path),
        Some("md" | "markdown") => {
            let text = fs::read_to_string(path)?;
            markdown::from_markdown(&text)
        }
        Some("svg") => Err(Error::Parse(
            "SVG is an export format; use a .mappa or .md file as input".to_owned(),
        )),
        Some(other) => Err(Error::Parse(format!(
            "unsupported input format: .{other} (expected .mappa, .json, .md or .markdown)"
        ))),
        None => Err(Error::Parse(
            "the input file has no extension; use .mappa, .json, .md or .markdown".to_owned(),
        )),
    }
}

/// Saves a map, choosing the format from the file extension.
///
/// Supported outputs: `.mappa`/`.json` (native) and `.md`/`.markdown` (lossy
/// outline). SVG output is produced by [`crate::export::svg`].
pub fn save_path(path: &Path, map: &MindMap) -> Result<()> {
    map.validate()?;
    match extension(path).as_deref() {
        Some("mappa" | "json") => json::save(path, map),
        Some("md" | "markdown") => {
            let text = markdown::to_markdown(map);
            json::write_atomic(path, text.as_bytes())
        }
        Some("svg") => Err(Error::Parse(
            "use `mappamente render` (or export::svg) to write SVG files".to_owned(),
        )),
        Some(other) => Err(Error::Parse(format!(
            "unsupported output format: .{other} (expected .mappa, .json, .md or .markdown)"
        ))),
        None => Err(Error::Parse(
            "the output file has no extension; use .mappa, .json, .md or .markdown".to_owned(),
        )),
    }
}

/// Whether the extension is a readable map format.
#[must_use]
pub fn is_supported_map_file(path: &Path) -> bool {
    matches!(
        extension(path).as_deref(),
        Some("mappa" | "json" | "md" | "markdown")
    )
}

/// Lowercase extension without the dot.
#[must_use]
pub fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-io-{label}-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn loads_and_saves_both_formats() {
        let directory = temporary_directory("formats");
        let mut map = MindMap::new("Formats", "Root");
        map.add_child(&map.root.id.clone(), "Child").unwrap();

        for name in ["map.mappa", "map.md"] {
            let path = directory.join(name);
            save_path(&path, &map).unwrap();
            let loaded = load_path(&path).unwrap();
            assert_eq!(loaded.outline(), map.outline(), "failed for {name}");
        }

        let markdown = fs::read_to_string(directory.join("map.md")).unwrap();
        assert!(markdown.starts_with("# Formats"));

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn unsupported_extensions_are_reported() {
        let directory = temporary_directory("unsupported");
        let path = directory.join("map.txt");
        let map = MindMap::new("Test", "Root");
        assert!(save_path(&path, &map).is_err());
        assert!(load_path(&path).is_err());
        assert!(!is_supported_map_file(&path));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn svg_has_a_dedicated_hint() {
        let map = MindMap::new("Test", "Root");
        let error = save_path(Path::new("out.svg"), &map).unwrap_err();
        assert!(error.to_string().contains("mappamente render"));
        assert!(load_path(Path::new("in.svg")).is_err());
    }

    #[test]
    fn detects_supported_files() {
        for path in ["a.mappa", "b.JSON", "c.md", "d.MARKDOWN"] {
            assert!(is_supported_map_file(Path::new(path)), "{path}");
        }
        assert!(!is_supported_map_file(Path::new("e.svg")));
        assert!(!is_supported_map_file(Path::new("f")));
    }
}
