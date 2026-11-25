//! Undo/redo built on document snapshots.
//!
//! Mind maps are small (hundreds of nodes), so cloning the whole document per
//! edit is simpler and far less error-prone than inverse commands. Edits are
//! grouped in transactions: a GUI drag, a paste or a whole AI generation can
//! become a single undo step.

use crate::error::Result;
use crate::model::MindMap;

#[derive(Clone, Debug)]
struct Snapshot {
    map: MindMap,
    rev: u64,
    label: String,
}

/// A document together with its undo history.
#[derive(Debug)]
pub struct Editor {
    map: MindMap,
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    rev: u64,
    next_rev: u64,
    saved_rev: Option<u64>,
    limit: usize,
    last_label: Option<String>,
    last_at_ms: u64,
}

impl Editor {
    /// Wraps a map in an editor with a clean history.
    #[must_use]
    pub fn new(map: MindMap) -> Self {
        Self::with_limit(map, 200)
    }

    /// Same as [`Editor::new`] with a custom history limit.
    #[must_use]
    pub fn with_limit(map: MindMap, limit: usize) -> Self {
        Self {
            map,
            past: Vec::new(),
            future: Vec::new(),
            rev: 0,
            next_rev: 1,
            saved_rev: Some(0),
            limit: limit.max(1),
            last_label: None,
            last_at_ms: 0,
        }
    }

    /// Read-only access to the current document.
    #[must_use]
    pub fn map(&self) -> &MindMap {
        &self.map
    }

    /// Consumes the editor and returns the document.
    #[must_use]
    pub fn into_map(self) -> MindMap {
        self.map
    }

    /// Applies a mutation as one undo step.
    ///
    /// If `change` returns an error the document is restored exactly and the
    /// history is left untouched, so callers can compose several operations
    /// without worrying about partial state.
    pub fn apply<F, T>(&mut self, label: &str, change: F) -> Result<T>
    where
        F: FnOnce(&mut MindMap) -> Result<T>,
    {
        self.run(label, 0, None, change)
    }

    /// Applies a mutation, merging it into the previous one when it has the
    /// same `label` and happened within `window_ms`.
    ///
    /// This is meant for continuous interactions such as dragging a node: the
    /// first frame creates an undo checkpoint and the following frames reuse
    /// it, so a whole drag is undone in one step.
    pub fn apply_coalesced<F, T>(
        &mut self,
        label: &str,
        now_ms: u64,
        window_ms: u64,
        change: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut MindMap) -> Result<T>,
    {
        self.run(label, now_ms, Some(window_ms), change)
    }

    fn run<F, T>(
        &mut self,
        label: &str,
        now_ms: u64,
        coalesce_window: Option<u64>,
        change: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut MindMap) -> Result<T>,
    {
        let before = self.map.clone();
        let before_rev = self.rev;

        match change(&mut self.map) {
            Ok(value) => {
                let coalesce = coalesce_window.is_some_and(|window| {
                    window > 0
                        && self.last_label.as_deref() == Some(label)
                        && now_ms.saturating_sub(self.last_at_ms) < window
                });
                if !coalesce {
                    self.past.push(Snapshot {
                        map: before,
                        rev: before_rev,
                        label: label.to_owned(),
                    });
                    if self.past.len() > self.limit {
                        self.past.remove(0);
                    }
                }
                self.future.clear();
                self.rev = self.next_rev;
                self.next_rev += 1;
                self.map.touch();
                self.last_label = coalesce_window.map(|_| label.to_owned());
                self.last_at_ms = now_ms;
                Ok(value)
            }
            Err(error) => {
                self.map = before;
                Err(error)
            }
        }
    }

    /// Reverts the last edit. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.past.pop() else {
            return false;
        };
        let Snapshot { map, rev, label } = snapshot;
        self.future.push(Snapshot {
            map: std::mem::replace(&mut self.map, map),
            rev: self.rev,
            label,
        });
        self.rev = rev;
        self.last_label = None;
        true
    }

    /// Re-applies the last undone edit. Returns `false` when there is nothing
    /// to redo.
    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.future.pop() else {
            return false;
        };
        let Snapshot { map, rev, label } = snapshot;
        self.past.push(Snapshot {
            map: std::mem::replace(&mut self.map, map),
            rev: self.rev,
            label,
        });
        self.rev = rev;
        self.last_label = None;
        true
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Number of available undo steps.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// Label of the next undo step, useful for menu entries.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|snapshot| snapshot.label.as_str())
    }

    /// Marks the current revision as persisted.
    pub fn mark_saved(&mut self) {
        self.saved_rev = Some(self.rev);
    }

    /// Marks the document as not persisted, e.g. after "New".
    pub fn mark_dirty(&mut self) {
        self.saved_rev = None;
    }

    /// Whether the document differs from the last saved revision.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.saved_rev != Some(self.rev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayoutKind, MindMap};

    fn map() -> MindMap {
        MindMap::new("History", "Root")
    }

    #[test]
    fn apply_undo_and_redo() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        assert!(!editor.can_undo());
        assert!(!editor.is_dirty());

        editor
            .apply("add", |map| map.add_child(&root, "A"))
            .unwrap();
        assert_eq!(editor.map().node_count(), 2);
        assert!(editor.can_undo());
        assert!(editor.is_dirty());
        assert_eq!(editor.undo_label(), Some("add"));

        assert!(editor.undo());
        assert_eq!(editor.map().node_count(), 1);
        assert!(editor.can_redo());
        assert!(!editor.is_dirty());

        assert!(editor.redo());
        assert_eq!(editor.map().node_count(), 2);
        assert!(!editor.can_redo());
        assert!(editor.is_dirty());

        assert!(!editor.redo());
    }

    #[test]
    fn dirty_tracks_the_saved_revision_across_undo() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();

        editor
            .apply("add A", |map| map.add_child(&root, "A"))
            .unwrap();
        editor.mark_saved();
        assert!(!editor.is_dirty());

        editor
            .apply("add B", |map| map.add_child(&root, "B"))
            .unwrap();
        assert!(editor.is_dirty());
        editor.undo();
        assert!(!editor.is_dirty());
        editor.undo();
        assert!(editor.is_dirty());

        // A new edit after undo must not reuse the saved revision number.
        editor
            .apply("add C", |map| map.add_child(&root, "C"))
            .unwrap();
        assert!(editor.is_dirty());
        assert!(!editor.can_redo());
    }

    #[test]
    fn failed_edit_rolls_back() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        let before = editor.map().clone();

        let result: Result<()> = editor.apply("broken", |map| {
            map.add_child(&root, "Temporary")?;
            Err(crate::error::Error::RootRemoval)
        });
        assert!(result.is_err());
        assert_eq!(editor.map(), &before);
        assert!(!editor.can_undo());
        assert!(!editor.is_dirty());
    }

    #[test]
    fn coalesced_edits_share_one_undo_step() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        let child = editor
            .apply("add", |map| map.add_child(&root, "A"))
            .unwrap();
        editor.mark_saved();

        for (index, now) in [1000, 1100, 1200].into_iter().enumerate() {
            editor
                .apply_coalesced("drag", now, 700, |map| {
                    map.set_pos(&child, Some([index as f32 * 10.0, 0.0]))
                })
                .unwrap();
        }
        assert_eq!(editor.undo_depth(), 2);

        editor.undo();
        assert_eq!(editor.map().find(&child).unwrap().pos, None);
    }

    #[test]
    fn coalescing_stops_after_the_window() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        let child = editor
            .apply("add", |map| map.add_child(&root, "A"))
            .unwrap();

        editor
            .apply_coalesced("drag", 1000, 700, |map| {
                map.set_pos(&child, Some([10.0, 0.0]))
            })
            .unwrap();
        editor
            .apply_coalesced("drag", 2000, 700, |map| {
                map.set_pos(&child, Some([20.0, 0.0]))
            })
            .unwrap();

        assert_eq!(editor.undo_depth(), 3);
        editor.undo();
        assert_eq!(editor.map().find(&child).unwrap().pos, Some([10.0, 0.0]));
    }

    #[test]
    fn different_labels_do_not_coalesce() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        editor
            .apply_coalesced("move", 1000, 700, |map| map.add_child(&root, "A"))
            .unwrap();
        editor
            .apply_coalesced("edit text", 1050, 700, |map| map.set_text(&root, "New"))
            .unwrap();
        assert_eq!(editor.undo_depth(), 2);
    }

    #[test]
    fn discrete_edits_never_coalesce() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        editor
            .apply("move", |map| map.add_child(&root, "A"))
            .unwrap();
        editor
            .apply_coalesced("move", 1000, 700, |map| map.set_text(&root, "New"))
            .unwrap();
        assert_eq!(editor.undo_depth(), 2);
    }

    #[test]
    fn history_limit_drops_oldest_snapshots() {
        let mut editor = Editor::with_limit(map(), 2);
        let root = editor.map().root.id.clone();
        for index in 0..5 {
            editor
                .apply("add", |map| map.add_child(&root, format!("Node {index}")))
                .unwrap();
        }
        assert_eq!(editor.undo_depth(), 2);
        assert!(editor.undo());
        assert!(editor.undo());
        assert!(!editor.undo());
        assert_eq!(editor.map().node_count(), 4);
    }

    #[test]
    fn transaction_groups_many_operations() {
        let mut editor = Editor::new(map());
        let root = editor.map().root.id.clone();
        editor
            .apply("generate", |map| {
                for index in 0..10 {
                    let id = map.add_child(&root, format!("Node {index}"))?;
                    map.add_child(&id, format!("Leaf {index}"))?;
                }
                map.set_layout(LayoutKind::Logical);
                Ok(())
            })
            .unwrap();
        assert_eq!(editor.map().node_count(), 21);
        assert_eq!(editor.undo_depth(), 1);
        editor.undo();
        assert_eq!(editor.map().node_count(), 1);
        assert_eq!(editor.map().layout, LayoutKind::Radial);
    }

    #[test]
    fn new_document_can_be_marked_dirty() {
        let mut editor = Editor::new(map());
        assert!(!editor.is_dirty());
        editor.mark_dirty();
        assert!(editor.is_dirty());
        editor.mark_saved();
        assert!(!editor.is_dirty());
    }
}
