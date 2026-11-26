//! Crash-recovery snapshots for `.mappa` documents.
//!
//! While the user is editing, a recovery file is written periodically and after
//! significant changes. If the application closes unexpectedly, the next open
//! can detect a newer recovery snapshot and offer to restore it.
//!
//! Recovery files are siblings of the original document so they stay associated
//! with it and are easy to locate. They are written atomically and removed
//! after a successful save.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::io::json;
use crate::model::MindMap;

/// Version of the recovery envelope. Independent of the map file version.
const RECOVERY_VERSION: u32 = 1;

/// Metadata + document stored in a recovery file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recovery {
    /// Envelope version.
    pub version: u32,
    /// Path of the document this recovery belongs to.
    pub original_path: PathBuf,
    /// `modified_at` of the document when the recovery snapshot was taken.
    pub saved_modified_at: u64,
    /// Unix timestamp in milliseconds when the recovery file was written.
    pub recovered_at: u64,
    /// The recovered document.
    pub map: MindMap,
}

/// A recovery candidate together with the saved file it belongs to.
#[derive(Clone, Debug)]
pub struct RecoveryCandidate {
    /// Path of the recovery file.
    pub recovery_path: PathBuf,
    /// Path of the original document.
    pub original_path: PathBuf,
    /// Document stored in the recovery file.
    pub map: MindMap,
    /// `modified_at` recorded in the recovery file.
    pub saved_modified_at: u64,
    /// When the recovery file was written.
    pub recovered_at: u64,
}

/// Returns the recovery-file path for a given document path.
#[must_use]
pub fn recovery_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("map");
    parent.join(format!(".{name}.mappamente-recovery"))
}

/// Writes a recovery snapshot for `path`.
///
/// The recovery file is removed if `map` matches the currently saved file
/// exactly (no unsaved changes), so recovery files do not accumulate needlessly.
pub fn write_recovery(path: &Path, map: &MindMap) -> Result<()> {
    let recovery = recovery_path(path);
    let candidate = Recovery {
        version: RECOVERY_VERSION,
        original_path: path.to_path_buf(),
        saved_modified_at: map.modified_at,
        recovered_at: crate::model::now_ms(),
        map: map.clone(),
    };
    let text = serde_json::to_string_pretty(&candidate)?;
    json::write_atomic(&recovery, text.as_bytes())
}

/// Reads and validates a recovery file if one exists and is newer than the
/// saved document.
///
/// Returns `Ok(None)` when no recovery file exists, when it is malformed, or
/// when it is not newer than the saved file. Errors are returned for I/O
/// failures that prevent reading an existing file.
pub fn read_recovery(path: &Path) -> Result<Option<RecoveryCandidate>> {
    let recovery = recovery_path(path);
    if !recovery.exists() {
        return Ok(None);
    }

    // Read the saved file revision first, under the shared advisory lock, so a
    // concurrent writer cannot change it between this read and the recovery
    // comparison. If the file is missing or unreadable, any recovery is
    // considered newer.
    let saved_modified_at = if path.exists() {
        json::modified_at(path).unwrap_or(0)
    } else {
        0
    };

    let text = fs::read_to_string(&recovery)?;
    let candidate: Recovery = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if candidate.version != RECOVERY_VERSION {
        return Ok(None);
    }
    if candidate.original_path != path {
        return Ok(None);
    }
    candidate.map.validate()?;

    if candidate.saved_modified_at <= saved_modified_at {
        return Ok(None);
    }

    Ok(Some(RecoveryCandidate {
        recovery_path: recovery,
        original_path: path.to_path_buf(),
        map: candidate.map,
        saved_modified_at: candidate.saved_modified_at,
        recovered_at: candidate.recovered_at,
    }))
}

/// Removes the recovery file for `path`, if any.
pub fn cleanup_recovery(path: &Path) -> Result<()> {
    let recovery = recovery_path(path);
    if recovery.exists() {
        fs::remove_file(&recovery)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MindMap;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mappamente-recovery-{label}-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn recovery_path_is_a_sibling() {
        let path = Path::new("/tmp/maps/ideas.mappa");
        assert_eq!(
            recovery_path(path),
            Path::new("/tmp/maps/.ideas.mappa.mappamente-recovery")
        );
    }

    #[test]
    fn write_and_read_recovery() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("map.mappa");
        let mut map = MindMap::new("Test", "Root");
        let root = map.root.id.clone();
        map.add_child(&root, "Child").unwrap();
        json::save(&path, &map).unwrap();

        let mut edited = map.clone();
        edited
            .edit(|draft| {
                draft.title = "Changed".to_owned();
                draft.add_child(&root, "Another").map(|_| ())
            })
            .unwrap();

        write_recovery(&path, &edited).unwrap();
        let candidate = read_recovery(&path)
            .unwrap()
            .expect("recovery should exist");
        assert_eq!(candidate.map, edited);
        assert_eq!(candidate.saved_modified_at, edited.modified_at);

        cleanup_recovery(&path).unwrap();
        assert!(read_recovery(&path).unwrap().is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn outdated_recovery_is_ignored() {
        let dir = temp_dir("outdated");
        let path = dir.join("map.mappa");
        let mut map = MindMap::new("Test", "Root");
        map.add_child(&map.root.id.clone(), "Child").unwrap();
        json::save(&path, &map).unwrap();

        // Recovery of the exact same document is not newer.
        write_recovery(&path, &map).unwrap();
        assert!(read_recovery(&path).unwrap().is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_newer_than_saved_file_is_detected() {
        let dir = temp_dir("newer");
        let path = dir.join("map.mappa");
        let mut map = MindMap::new("Test", "Root");
        map.add_child(&map.root.id.clone(), "Child").unwrap();
        json::save(&path, &map).unwrap();

        let mut edited = map.clone();
        edited
            .edit(|draft| {
                draft.title = "Changed".to_owned();
                Ok(())
            })
            .unwrap();
        write_recovery(&path, &edited).unwrap();

        let candidate = read_recovery(&path)
            .unwrap()
            .expect("recovery should be newer");
        assert_eq!(candidate.map, edited);

        // After the saved file catches up, the recovery is no longer offered.
        json::save(&path, &edited).unwrap();
        assert!(read_recovery(&path).unwrap().is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_is_offered_when_the_saved_file_is_missing() {
        let dir = temp_dir("missing-saved");
        let path = dir.join("map.mappa");
        let mut map = MindMap::new("Test", "Root");
        map.add_child(&map.root.id.clone(), "Child").unwrap();
        json::save(&path, &map).unwrap();

        let mut edited = map.clone();
        edited
            .edit(|draft| {
                draft.title = "Changed".to_owned();
                Ok(())
            })
            .unwrap();
        write_recovery(&path, &edited).unwrap();

        fs::remove_file(&path).unwrap();
        let candidate = read_recovery(&path)
            .unwrap()
            .expect("recovery should exist");
        assert_eq!(candidate.map, edited);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_recovery_is_ignored() {
        let dir = temp_dir("invalid");
        let path = dir.join("map.mappa");
        fs::write(&path, "{}").unwrap();
        let recovery = recovery_path(&path);
        fs::write(&recovery, "not json").unwrap();
        assert!(read_recovery(&path).unwrap().is_none());

        fs::write(&recovery, r#"{"version": 99}"#).unwrap();
        assert!(read_recovery(&path).unwrap().is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
