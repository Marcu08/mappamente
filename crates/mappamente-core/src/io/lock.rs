//! Cross-process advisory file locking.
//!
//! The lock is advisory: every writer (GUI and MCP) must acquire it before
//! reading or writing the map. The lock file itself is a sibling of the target
//! file and is created on demand. It is released automatically when the guard
//! is dropped.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::error::{Error, Result};

/// An advisory lock held for the lifetime of this value.
pub struct FileLock {
    _file: File,
    #[allow(dead_code)]
    path: PathBuf,
}

impl FileLock {
    /// Acquires an exclusive (writer) lock on `target`.
    ///
    /// Blocks until the lock is available. The lock is associated with a
    /// sibling lock file so it works even when `target` does not exist yet.
    pub fn exclusive(target: &Path) -> Result<Self> {
        let path = lock_path(target);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        file.lock_exclusive()
            .map_err(|error| Error::Io(std::io::Error::new(error.kind(), error.to_string())))?;
        Ok(Self { _file: file, path })
    }

    /// Acquires a shared (reader) lock on `target`.
    pub fn shared(target: &Path) -> Result<Self> {
        let path = lock_path(target);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        file.lock_shared()
            .map_err(|error| Error::Io(std::io::Error::new(error.kind(), error.to_string())))?;
        Ok(Self { _file: file, path })
    }
}

fn lock_path(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("map");
    parent.join(format!(".{name}.mappamente-lock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_path_is_a_sibling() {
        let path = Path::new("/tmp/maps/ideas.mappa");
        assert_eq!(
            lock_path(path),
            Path::new("/tmp/maps/.ideas.mappa.mappamente-lock")
        );
    }
}
