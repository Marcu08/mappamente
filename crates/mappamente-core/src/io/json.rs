//! Native `.mappa` persistence.
//!
//! The native format is pretty-printed JSON with a trailing newline and a
//! schema version. Writes go to a temporary file in the same directory and are
//! then renamed over the target, so a crash cannot leave a half-written map.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::model::MindMap;

/// Serializes a map, validating it first.
pub fn to_string(map: &MindMap) -> Result<String> {
    map.validate()?;
    let mut text = serde_json::to_string_pretty(map)?;
    text.push('\n');
    Ok(text)
}

/// Parses and validates a map from JSON.
pub fn from_str(text: &str) -> Result<MindMap> {
    let map: MindMap = serde_json::from_str(text)?;
    map.validate()?;
    Ok(map)
}

/// Loads a map from disk.
pub fn load(path: &Path) -> Result<MindMap> {
    let text = fs::read_to_string(path)?;
    from_str(&text)
}

/// Saves a map to disk atomically.
pub fn save(path: &Path, map: &MindMap) -> Result<()> {
    let text = to_string(map)?;
    write_atomic(path, text.as_bytes())
}

/// Writes `bytes` to `path` using a temporary file and a rename.
///
/// The rename is retried briefly on Windows, where another process (for
/// example a file watcher) may still hold the destination open.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Error::Io(std::io::Error::new(
                ErrorKind::InvalidInput,
                "the destination path has no valid file name",
            ))
        })?;

    let temporary = temporary_path(parent, name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;

    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    drop(file);

    let mut last_error = None;
    for attempt in 0..5 {
        match fs::rename(&temporary, path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt < 4 {
                    thread::sleep(Duration::from_millis(20 + 15 * attempt));
                }
            }
        }
    }

    let _ = fs::remove_file(&temporary);
    let Some(error) = last_error else {
        return Ok(());
    };
    Err(error.into())
}

fn temporary_path(parent: &Path, name: &str) -> PathBuf {
    parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        fastrand::u64(..)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayoutKind, Link, MindMap, Node};

    fn sample() -> MindMap {
        let mut map = MindMap::new("My map", "Root");
        let child = map.add_child(&map.root.id.clone(), "Child").unwrap();
        map.set_note(&child, "A note").unwrap();
        map.set_color(&child, Some("#ff8800")).unwrap();
        map.set_pos(&child, Some([12.5, -4.0])).unwrap();
        let grandchild = map.add_child(&child, "Grandchild").unwrap();
        map.set_collapsed(&child, true).unwrap();
        map.set_layout(LayoutKind::Logical);
        map.theme = "ocean".to_owned();
        map.links.push(Link {
            from: child,
            to: grandchild,
            label: Some("related".to_owned()),
        });
        map
    }

    #[test]
    fn json_roundtrip_preserves_everything() {
        let map = sample();
        let text = to_string(&map).unwrap();
        assert!(text.ends_with('\n'));
        let parsed = from_str(&text).unwrap();
        assert_eq!(parsed, map);
    }

    #[test]
    fn output_is_stable_and_pretty() {
        let map = sample();
        let first = to_string(&map).unwrap();
        let second = to_string(&map).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("{\n  \"version\": 1,"));
        assert!(first.contains("\"layout\": \"logical\""));
        assert!(first.contains("\"children\": ["));
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        let map = sample();
        let text = to_string(&map).unwrap();
        let with_extra = text.replacen('{', "{\n  \"future_field\": {\"x\": 1},", 1);
        let parsed = from_str(&with_extra).unwrap();
        assert_eq!(parsed, map);
    }

    #[test]
    fn unsupported_versions_are_rejected() {
        let map = sample();
        let text = to_string(&map).unwrap();
        let newer = text.replacen("\"version\": 1", "\"version\": 99", 1);
        assert!(matches!(
            from_str(&newer),
            Err(Error::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn invalid_colors_are_rejected_on_load() {
        let map = sample();
        let text = to_string(&map).unwrap();
        let broken = text.replacen("#ff8800", "orange", 1);
        assert!(matches!(from_str(&broken), Err(Error::InvalidColor(_))));
    }

    #[test]
    fn broken_json_reports_a_json_error() {
        assert!(matches!(from_str("{"), Err(Error::Json(_))));
    }

    #[test]
    fn save_and_load_overwrite_without_leftovers() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-test-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");

        let map = sample();
        save(&path, &map).unwrap();
        assert_eq!(load(&path).unwrap(), map);

        let mut changed = map.clone();
        changed.title = "Changed".to_owned();
        let root = changed.root.id.clone();
        changed.add_child(&root, "New").unwrap();
        save(&path, &changed).unwrap();
        assert_eq!(load(&path).unwrap(), changed);

        let leftovers: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn invalid_maps_cannot_be_saved() {
        let mut map = sample();
        map.root.children.push(Node {
            id: map.root.id.clone(),
            ..Node::new("duplicate")
        });
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-invalid-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("bad.mappa");
        assert!(matches!(save(&path, &map), Err(Error::DuplicateId(_))));
        assert!(!path.exists());
        fs::remove_dir_all(&directory).unwrap();
    }
}
