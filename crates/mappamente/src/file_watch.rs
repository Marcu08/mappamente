//! Cross-platform file watcher for the currently open map.
//!
//! Wraps `notify::RecommendedWatcher` and forwards events through an
//! unbounded channel. The GUI polls the channel each frame and decides whether
//! the event represents a real external change.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// A watcher that notifies the GUI when the open file changes on disk.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    receiver: Receiver<FileEvent>,
    watching: Option<PathBuf>,
}

/// Simplified event produced by the watcher.
#[derive(Clone, Debug)]
pub enum FileEvent {
    /// The file content likely changed.
    Changed(PathBuf),
    /// The file was removed or renamed away.
    Removed(PathBuf),
    /// An error occurred while watching.
    Error(String),
}

impl FileWatcher {
    /// Creates a new watcher with no active path.
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| match result {
                Ok(event) => {
                    let paths: Vec<_> = event.paths.into_iter().collect();
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            for path in paths {
                                let _ = sender.send(FileEvent::Changed(path));
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in paths {
                                let _ = sender.send(FileEvent::Removed(path));
                            }
                        }
                        _ => {}
                    }
                }
                Err(error) => {
                    let _ = sender.send(FileEvent::Error(error.to_string()));
                }
            },
            notify::Config::default(),
        )
        .expect("file watcher should initialize on supported platforms");

        Self {
            watcher,
            receiver,
            watching: None,
        }
    }

    /// Watches `path`, replacing any previous watch.
    pub fn watch(&mut self, path: &Path) {
        if self.watching.as_deref() == Some(path) {
            return;
        }
        // Stop watching the previous path so we do not accumulate watches.
        if let Some(old) = self.watching.take() {
            let _ = self.watcher.unwatch(old.parent().unwrap_or(&old));
        }
        // Drain stale events before switching paths.
        while self.receiver.try_recv().is_ok() {}
        let _ = self
            .watcher
            .watch(path.parent().unwrap_or(path), RecursiveMode::NonRecursive);
        self.watching = Some(path.to_path_buf());
    }

    /// Stops watching.
    pub fn clear(&mut self) {
        if let Some(path) = self.watching.take() {
            let _ = self.watcher.unwatch(path.parent().unwrap_or(&path));
        }
        while self.receiver.try_recv().is_ok() {}
    }

    /// Polls pending events.
    pub fn poll(&self) -> Vec<FileEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.clear();
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn watcher_reports_a_file_change() {
        let dir = std::env::temp_dir().join(format!(
            "mappamente-watch-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("map.mappa");
        fs::write(&path, "first").unwrap();

        let mut watcher = FileWatcher::new();
        watcher.watch(&path);

        // Allow the watcher to register.
        thread::sleep(Duration::from_millis(100));
        fs::write(&path, "second").unwrap();

        let mut found = false;
        for _ in 0..50 {
            for event in watcher.poll() {
                if let FileEvent::Changed(changed) = event
                    && changed == path
                {
                    found = true;
                }
            }
            if found {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        assert!(found, "watcher did not report the change");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_paths_tracks_the_new_target() {
        let dir = std::env::temp_dir().join(format!(
            "mappamente-watch-switch-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.mappa");
        let second = dir.join("second.mappa");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();

        let mut watcher = FileWatcher::new();
        watcher.watch(&first);
        thread::sleep(Duration::from_millis(100));
        watcher.watch(&second);
        assert_eq!(watcher.watching, Some(second.clone()));
        thread::sleep(Duration::from_millis(100));

        fs::write(&second, "changed").unwrap();
        let mut found = false;
        for _ in 0..50 {
            for event in watcher.poll() {
                if let FileEvent::Changed(changed) = event
                    && changed == second
                {
                    found = true;
                }
            }
            if found {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(found, "new path should emit events");

        watcher.clear();
        assert!(watcher.watching.is_none());

        // Wait for any in-flight events to drain, then write again and verify
        // no further events arrive. The watcher teardown is asynchronous on
        // some platforms, so we poll repeatedly until the queue stays empty.
        let mut empty_cycles = 0;
        for _ in 0..60 {
            if watcher.poll().is_empty() {
                empty_cycles += 1;
                if empty_cycles >= 5 {
                    break;
                }
            } else {
                empty_cycles = 0;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            empty_cycles >= 5,
            "previous watcher events should drain after clear"
        );

        fs::write(&second, "changed again").unwrap();
        empty_cycles = 0;
        for _ in 0..60 {
            if watcher.poll().is_empty() {
                empty_cycles += 1;
                if empty_cycles >= 5 {
                    break;
                }
            } else {
                empty_cycles = 0;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            empty_cycles >= 5,
            "clear should stop new events from the old watcher"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
