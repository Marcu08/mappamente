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

use serde_json::Value;

use crate::error::{Error, Result};
use crate::io::lock::FileLock;
use crate::model::{FILE_VERSION, MindMap};

/// Serializes a map, validating it first.
pub fn to_string(map: &MindMap) -> Result<String> {
    map.validate()?;
    let mut text = serde_json::to_string_pretty(map)?;
    text.push('\n');
    Ok(text)
}

/// Migrates a raw JSON document to the current schema version.
///
/// The migration point is deliberately singular: every load goes through it
/// before deserialization. Version 1 is the current schema and needs no
/// transformation; unknown versions are rejected with the found and supported
/// versions in the error.
pub fn migrate(value: Value) -> Result<Value> {
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Parse("missing or invalid `version` field".to_owned()))?;
    let version = u32::try_from(version)
        .map_err(|_| Error::Parse(format!("invalid file version: {version}")))?;
    match version {
        FILE_VERSION => Ok(value),
        other => Err(Error::UnsupportedVersion(other)),
    }
}

/// Parses, migrates and validates a map from JSON.
pub fn from_str(text: &str) -> Result<MindMap> {
    let value: Value = serde_json::from_str(text)?;
    let value = migrate(value)?;
    let map: MindMap = serde_json::from_value(value)?;
    map.validate()?;
    Ok(map)
}

/// Loads a map from disk.
///
/// Acquires an advisory shared lock so the read does not race with a writer
/// from another process.
pub fn load(path: &Path) -> Result<MindMap> {
    crate::io::ensure_file_size(path)?;
    let _lock = FileLock::shared(path)?;
    let text = fs::read_to_string(path)?;
    from_str(&text)
}

/// Reads the `modified_at` timestamp from a saved map without deserializing
/// the whole document.
pub fn modified_at(path: &Path) -> Result<u64> {
    crate::io::ensure_file_size(path)?;
    let _lock = FileLock::shared(path)?;
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    value
        .get("modified_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Parse("missing modified_at in saved file".to_owned()))
}

/// Result of an atomic compare-and-save operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveResult {
    /// The file was written and now has this revision.
    Saved(u64),
    /// The on-disk revision did not match the caller's expectation. The file
    /// was not modified.
    Conflict {
        /// Revision the caller expected, or `None` for a new file.
        expected: Option<u64>,
        /// Revision currently on disk, or `None` if the file no longer exists.
        actual: Option<u64>,
    },
}

/// Saves a map to disk atomically.
///
/// Acquires an advisory exclusive lock on the target so the write is isolated
/// from concurrent GUI or MCP processes. Callers that already hold the lock
/// should use [`save_unlocked`].
pub fn save(path: &Path, map: &MindMap) -> Result<()> {
    let _lock = FileLock::exclusive(path)?;
    save_unlocked(path, map)
}

/// Like [`save`] but does not acquire an advisory lock.
///
/// Useful when the caller already holds the lock and wants to avoid a
/// self-deadlock.
pub fn save_unlocked(path: &Path, map: &MindMap) -> Result<()> {
    let text = to_string(map)?;
    write_atomic(path, text.as_bytes())
}

/// Saves `map` to `path` only when the file is missing or its current
/// `modified_at` equals `expected_modified_at`.
///
/// The check and the write happen atomically under a single exclusive advisory
/// lock. This is the primitive the GUI uses to avoid overwriting external
/// changes.
pub fn save_if_current(
    path: &Path,
    map: &MindMap,
    expected_modified_at: Option<u64>,
) -> Result<SaveResult> {
    let _lock = FileLock::exclusive(path)?;
    let actual = if path.exists() {
        Some(read_modified_at_unlocked(path)?)
    } else {
        None
    };
    if actual != expected_modified_at {
        return Ok(SaveResult::Conflict {
            expected: expected_modified_at,
            actual,
        });
    }
    save_unlocked(path, map)?;
    Ok(SaveResult::Saved(map.modified_at))
}

fn read_modified_at_unlocked(path: &Path) -> Result<u64> {
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    value
        .get("modified_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Parse("missing modified_at in saved file".to_owned()))
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
    fn migrate_accepts_the_current_version_unchanged() {
        let value = serde_json::json!({"version": FILE_VERSION, "future": true});
        let migrated = migrate(value.clone()).unwrap();
        assert_eq!(migrated, value);
    }

    #[test]
    fn migrate_rejects_unknown_versions() {
        let value = serde_json::json!({"version": 99});
        assert!(matches!(migrate(value), Err(Error::UnsupportedVersion(99))));
    }

    #[test]
    fn migrate_rejects_missing_or_invalid_versions() {
        assert!(matches!(
            migrate(serde_json::json!({"title": "no version"})),
            Err(Error::Parse(_))
        ));
        assert!(matches!(
            migrate(serde_json::json!({"version": "1"})),
            Err(Error::Parse(_))
        ));
    }

    #[test]
    fn oversized_files_are_rejected_by_load() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-large-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("large.mappa");
        let file = fs::File::create(&path).unwrap();
        file.set_len(crate::io::MAX_MAP_FILE_BYTES + 1).unwrap();
        drop(file);

        assert!(matches!(
            load(&path),
            Err(Error::FileTooLarge { limit, .. }) if limit == crate::io::MAX_MAP_FILE_BYTES
        ));

        fs::remove_dir_all(&directory).unwrap();
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

    #[test]
    fn save_if_current_writes_new_file_when_expected_is_none() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-cas-new-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let map = sample();

        let result = save_if_current(&path, &map, None).unwrap();
        assert_eq!(result, SaveResult::Saved(map.modified_at));
        assert_eq!(load(&path).unwrap(), map);

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn save_if_current_rejects_existing_file_when_expected_is_none() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-cas-existing-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let existing = sample();
        save(&path, &existing).unwrap();
        let actual = modified_at(&path).unwrap();

        let map = sample();
        let result = save_if_current(&path, &map, None).unwrap();
        assert_eq!(
            result,
            SaveResult::Conflict {
                expected: None,
                actual: Some(actual),
            }
        );
        assert_eq!(load(&path).unwrap(), existing);

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn save_if_current_rejects_missing_file_when_expected_is_some() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-cas-missing-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let map = sample();

        let result = save_if_current(&path, &map, Some(1234)).unwrap();
        assert_eq!(
            result,
            SaveResult::Conflict {
                expected: Some(1234),
                actual: None,
            }
        );
        assert!(!path.exists());

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn save_if_current_succeeds_when_revision_matches() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-cas-match-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let mut map = sample();
        save(&path, &map).unwrap();
        let base = modified_at(&path).unwrap();

        map.edit(|draft| {
            draft.title = "Updated".to_owned();
            Ok(())
        })
        .unwrap();

        let result = save_if_current(&path, &map, Some(base)).unwrap();
        assert_eq!(result, SaveResult::Saved(map.modified_at));
        assert_eq!(load(&path).unwrap(), map);

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn save_if_current_rejects_stale_revision() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-cas-stale-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let mut map = sample();
        save(&path, &map).unwrap();
        let base = modified_at(&path).unwrap();

        // An external writer advances the on-disk revision.
        let mut external = map.clone();
        external
            .edit(|draft| {
                draft.title = "External".to_owned();
                Ok(())
            })
            .unwrap();
        save(&path, &external).unwrap();
        let actual = modified_at(&path).unwrap();

        map.edit(|draft| {
            draft.title = "Local".to_owned();
            Ok(())
        })
        .unwrap();
        let result = save_if_current(&path, &map, Some(base)).unwrap();
        assert_eq!(
            result,
            SaveResult::Conflict {
                expected: Some(base),
                actual: Some(actual),
            }
        );
        // The file on disk must remain the external version.
        assert_eq!(load(&path).unwrap(), external);

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn modified_at_reads_timestamp_without_full_load() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-ts-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let mut map = sample();
        map.edit(|draft| {
            draft.title = "Timestamped".to_owned();
            Ok(())
        })
        .unwrap();
        save(&path, &map).unwrap();
        assert_eq!(modified_at(&path).unwrap(), map.modified_at);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn concurrent_saves_keep_the_file_valid() {
        let directory = std::env::temp_dir().join(format!(
            "mappamente-json-concurrent-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.mappa");
        let map = sample();
        save(&path, &map).unwrap();

        let path = std::sync::Arc::new(path);
        let mut handles = Vec::new();
        for index in 0..10 {
            let path = std::sync::Arc::clone(&path);
            handles.push(std::thread::spawn(move || {
                let mut map = load(&path).unwrap();
                let root = map.root.id.clone();
                map.edit(|draft| {
                    draft.add_child(&root, format!("Concurrent {index}"))?;
                    Ok(())
                })
                .unwrap();
                save(&path, &map).unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        let map = load(&path).unwrap();
        assert!(map.validate().is_ok());
        fs::remove_dir_all(&directory).unwrap();
    }
}
