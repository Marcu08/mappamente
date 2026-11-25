//! Document model for `.mappa` files.
//!
//! A mind map is a single tree of [`Node`]s. Nodes have stable, short ids so
//! that external tools (the MCP server, the AI panel) can address them without
//! sending whole subtrees back and forth.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Version written to new files and accepted when loading.
pub const FILE_VERSION: u32 = 1;

/// Native file extension, without the dot.
pub const FILE_EXTENSION: &str = "mappa";

/// Identifier of a node. Generated ids look like `n_7k3m9q2x`.
pub type NodeId = String;

/// Algorithm used to place nodes when no manual position is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutKind {
    /// Bilateral mind map: branches alternate on the left and right of the root.
    Tree,
    /// Classic radial layout: branches spread around the root.
    #[default]
    Radial,
    /// All branches grow to the right of the root.
    Logical,
}

/// A single node (topic) of the map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub text: String,
    /// Optional longer note. Not shown in the main canvas.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Optional accent color in `#RRGGBB` or `#RRGGBBAA` form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Whether the children are hidden in the editor.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Manual top-left position override. `None` means "let the layout decide".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 2]>,
    #[serde(default)]
    pub children: Vec<Node>,
}

impl Node {
    /// Creates a detached node with a fresh id.
    ///
    /// Ids are regenerated when the node is inserted into a map, so two
    /// detached nodes can never conflict.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: fresh_id('n'),
            text: text.into(),
            note: String::new(),
            color: None,
            collapsed: false,
            pos: None,
            children: Vec::new(),
        }
    }

    /// Number of nodes in this subtree, including `self`.
    #[must_use]
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(Node::node_count).sum::<usize>()
    }
}

/// Reserved for cross-links between branches (not rendered yet).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub from: NodeId,
    pub to: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A complete mind map document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MindMap {
    /// Format version. Always [`FILE_VERSION`] for documents written by this
    /// build.
    pub version: u32,
    /// Stable document id.
    pub id: String,
    pub title: String,
    /// Built-in theme name (`dark`, `light`, `paper`, `ocean`).
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub layout: LayoutKind,
    /// Unix timestamp in milliseconds.
    pub created_at: u64,
    /// Unix timestamp in milliseconds.
    pub modified_at: u64,
    pub root: Node,
    /// Reserved for future cross-branch links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
}

impl MindMap {
    /// Creates a map with a single root node.
    #[must_use]
    pub fn new(title: impl Into<String>, root_text: impl Into<String>) -> Self {
        let now = now_ms();
        Self {
            version: FILE_VERSION,
            id: fresh_id('m'),
            title: title.into(),
            theme: default_theme(),
            layout: LayoutKind::default(),
            created_at: now,
            modified_at: now,
            root: Node::new(root_text),
            links: Vec::new(),
        }
    }

    /// Immutable lookup by id.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Node> {
        find_in(&self.root, id)
    }

    /// Mutable lookup by id.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut Node> {
        find_mut_in(&mut self.root, id)
    }

    /// Returns the parent of `id`, or `None` for the root and missing ids.
    #[must_use]
    pub fn parent_of(&self, id: &str) -> Option<&Node> {
        find_parent_in(&self.root, id)
    }

    /// Returns the depth of a node (root is `0`).
    #[must_use]
    pub fn depth_of(&self, id: &str) -> Option<usize> {
        depth_in(&self.root, id, 0)
    }

    /// Total number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.root.node_count()
    }

    /// Appends a new child and returns its id.
    pub fn add_child(&mut self, parent: &str, text: impl Into<String>) -> Result<NodeId> {
        self.add_child_at(parent, usize::MAX, text)
    }

    /// Inserts a new child at `index` (clamped to the number of children).
    pub fn add_child_at(
        &mut self,
        parent: &str,
        index: usize,
        text: impl Into<String>,
    ) -> Result<NodeId> {
        let id = self.fresh_node_id();
        let parent = self
            .find_mut(parent)
            .ok_or_else(|| Error::NodeNotFound(parent.to_owned()))?;
        let idx = index.min(parent.children.len());
        parent.children.insert(
            idx,
            Node {
                id: id.clone(),
                text: text.into(),
                note: String::new(),
                color: None,
                collapsed: false,
                pos: None,
                children: Vec::new(),
            },
        );
        Ok(id)
    }

    /// Inserts an existing subtree below `parent`.
    ///
    /// All ids in the subtree are regenerated to keep the map free of
    /// duplicates. Returns the id of the inserted subtree root.
    pub fn insert_subtree(
        &mut self,
        parent: &str,
        index: usize,
        mut subtree: Node,
    ) -> Result<NodeId> {
        let mut used = HashSet::new();
        collect_ids(&self.root, &mut used);
        assign_fresh_ids(&mut subtree, &mut used);
        let inserted_id = subtree.id.clone();

        let parent = self
            .find_mut(parent)
            .ok_or_else(|| Error::NodeNotFound(parent.to_owned()))?;
        let idx = index.min(parent.children.len());
        parent.children.insert(idx, subtree);
        Ok(inserted_id)
    }

    /// Detaches and returns a subtree. The root cannot be removed.
    pub fn remove(&mut self, id: &str) -> Result<Node> {
        if id == self.root.id {
            return Err(Error::RootRemoval);
        }
        remove_in(&mut self.root, id).ok_or_else(|| Error::NodeNotFound(id.to_owned()))
    }

    /// Moves a node below `new_parent` at `index` (clamped).
    ///
    /// `index` is interpreted after the node has been detached. Moving a node
    /// inside its own subtree is rejected.
    pub fn move_node(&mut self, id: &str, new_parent: &str, index: usize) -> Result<()> {
        if id == self.root.id {
            return Err(Error::RootMove);
        }
        let moving = self
            .find(id)
            .ok_or_else(|| Error::NodeNotFound(id.to_owned()))?;
        if self.find(new_parent).is_none() {
            return Err(Error::NodeNotFound(new_parent.to_owned()));
        }
        // Moving a branch below one of its own descendants would detach it
        // from the tree.
        if contains_id(moving, new_parent) {
            return Err(Error::Cycle(id.to_owned()));
        }

        let node =
            remove_in(&mut self.root, id).ok_or_else(|| Error::NodeNotFound(id.to_owned()))?;
        // `new_parent` cannot be inside `node`: checked above.
        let parent = self
            .find_mut(new_parent)
            .ok_or_else(|| Error::NodeNotFound(new_parent.to_owned()))?;
        let idx = index.min(parent.children.len());
        parent.children.insert(idx, node);
        Ok(())
    }

    /// Replaces the label of a node.
    pub fn set_text(&mut self, id: &str, text: impl Into<String>) -> Result<()> {
        self.find_mut(id)
            .ok_or_else(|| Error::NodeNotFound(id.to_owned()))?
            .text = text.into();
        Ok(())
    }

    /// Replaces the note of a node.
    pub fn set_note(&mut self, id: &str, note: impl Into<String>) -> Result<()> {
        self.find_mut(id)
            .ok_or_else(|| Error::NodeNotFound(id.to_owned()))?
            .note = note.into();
        Ok(())
    }

    /// Sets or clears the accent color.
    pub fn set_color(&mut self, id: &str, color: Option<&str>) -> Result<()> {
        if let Some(color) = color
            && !is_valid_color(color)
        {
            return Err(Error::InvalidColor(color.to_owned()));
        }
        self.find_mut(id)
            .ok_or_else(|| Error::NodeNotFound(id.to_owned()))?
            .color = color.map(str::to_owned);
        Ok(())
    }

    /// Collapses or expands a branch.
    pub fn set_collapsed(&mut self, id: &str, collapsed: bool) -> Result<()> {
        self.find_mut(id)
            .ok_or_else(|| Error::NodeNotFound(id.to_owned()))?
            .collapsed = collapsed;
        Ok(())
    }

    /// Sets or clears the manual position override.
    pub fn set_pos(&mut self, id: &str, pos: Option<[f32; 2]>) -> Result<()> {
        if let Some([x, y]) = pos
            && (!x.is_finite() || !y.is_finite())
        {
            return Err(Error::InvalidPosition);
        }
        self.find_mut(id)
            .ok_or_else(|| Error::NodeNotFound(id.to_owned()))?
            .pos = pos;
        Ok(())
    }

    /// Changes the layout algorithm.
    pub fn set_layout(&mut self, layout: LayoutKind) {
        self.layout = layout;
    }

    /// Updates the modification timestamp.
    pub fn touch(&mut self) {
        self.modified_at = now_ms();
    }

    /// Renders the tree as an indented Markdown outline (no title).
    #[must_use]
    pub fn outline(&self) -> String {
        fn walk(node: &Node, depth: usize, out: &mut String) {
            for _ in 0..depth {
                out.push_str("  ");
            }
            out.push_str("- ");
            for ch in node.text.chars() {
                out.push(if ch == '\n' || ch == '\r' { ' ' } else { ch });
            }
            out.push('\n');
            for child in &node.children {
                walk(child, depth + 1, out);
            }
        }

        let mut out = String::new();
        walk(&self.root, 0, &mut out);
        while out.ends_with('\n') {
            out.pop();
        }
        out
    }

    /// Checks structural invariants of the document.
    pub fn validate(&self) -> Result<()> {
        if self.version != FILE_VERSION {
            return Err(Error::UnsupportedVersion(self.version));
        }
        if !is_valid_id(&self.id) {
            return Err(Error::InvalidId(self.id.clone()));
        }

        let mut seen = HashSet::new();
        validate_node(&self.root, &mut seen)?;
        for link in &self.links {
            if !seen.contains(link.from.as_str()) || !seen.contains(link.to.as_str()) {
                return Err(Error::InvalidLink(link.from.clone(), link.to.clone()));
            }
        }
        Ok(())
    }

    fn fresh_node_id(&self) -> NodeId {
        loop {
            let id = fresh_id('n');
            if !self.contains(&id) {
                return id;
            }
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.find(id).is_some()
    }
}

/// Returns `true` for ids made of 1-64 ASCII letters, digits, `_` or `-`.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Returns `true` for `#RRGGBB` and `#RRGGBBAA` strings.
#[must_use]
pub fn is_valid_color(color: &str) -> bool {
    let bytes = color.as_bytes();
    (bytes.len() == 7 || bytes.len() == 9)
        && bytes[0] == b'#'
        && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

fn default_theme() -> String {
    "dark".to_owned()
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Milliseconds since the Unix epoch, saturating at `u64::MAX`.
#[must_use]
pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// 32 symbols: digits plus lowercase letters without the ambiguous `i l o u`.
const ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Generates a short, URL-safe id: one prefix letter, `_`, and 8 base32 chars.
fn fresh_id(prefix: char) -> NodeId {
    // 40 bits of entropy, enough for hundreds of thousands of nodes.
    let value = fastrand::u64(..(1u64 << 40));
    let mut id = String::with_capacity(10);
    id.push(prefix);
    id.push('_');
    for shift in (0..8).rev() {
        let symbol = ID_ALPHABET[((value >> (shift * 5)) & 0x1f) as usize];
        id.push(char::from(symbol));
    }
    id
}

fn find_in<'a>(node: &'a Node, id: &str) -> Option<&'a Node> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_in(child, id))
}

fn find_mut_in<'a>(node: &'a mut Node, id: &str) -> Option<&'a mut Node> {
    if node.id == id {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_mut_in(child, id))
}

fn find_parent_in<'a>(node: &'a Node, id: &str) -> Option<&'a Node> {
    if node.children.iter().any(|child| child.id == id) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_parent_in(child, id))
}

fn depth_in(node: &Node, id: &str, depth: usize) -> Option<usize> {
    if node.id == id {
        return Some(depth);
    }
    node.children
        .iter()
        .find_map(|child| depth_in(child, id, depth + 1))
}

fn contains_id(node: &Node, id: &str) -> bool {
    node.id == id || node.children.iter().any(|child| contains_id(child, id))
}

fn remove_in(node: &mut Node, id: &str) -> Option<Node> {
    if let Some(index) = node.children.iter().position(|child| child.id == id) {
        return Some(node.children.remove(index));
    }
    node.children
        .iter_mut()
        .find_map(|child| remove_in(child, id))
}

fn collect_ids(node: &Node, used: &mut HashSet<NodeId>) {
    used.insert(node.id.clone());
    for child in &node.children {
        collect_ids(child, used);
    }
}

fn assign_fresh_ids(node: &mut Node, used: &mut HashSet<NodeId>) {
    loop {
        let id = fresh_id('n');
        if used.insert(id.clone()) {
            node.id = id;
            break;
        }
    }
    for child in &mut node.children {
        assign_fresh_ids(child, used);
    }
}

fn validate_node<'a>(node: &'a Node, seen: &mut HashSet<&'a str>) -> Result<()> {
    if !is_valid_id(&node.id) {
        return Err(Error::InvalidId(node.id.clone()));
    }
    if !seen.insert(node.id.as_str()) {
        return Err(Error::DuplicateId(node.id.clone()));
    }
    if let Some(color) = &node.color
        && !is_valid_color(color)
    {
        return Err(Error::InvalidColor(color.clone()));
    }
    if let Some([x, y]) = node.pos
        && (!x.is_finite() || !y.is_finite())
    {
        return Err(Error::InvalidPosition);
    }
    for child in &node.children {
        validate_node(child, seen)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with_children() -> (MindMap, NodeId, NodeId) {
        let mut map = MindMap::new("Test", "Root");
        let a = map.add_child(&map.root.id.clone(), "A").unwrap();
        let b = map.add_child(&map.root.id.clone(), "B").unwrap();
        (map, a, b)
    }

    #[test]
    fn new_map_has_a_valid_root() {
        let map = MindMap::new("Title", "Root");
        assert_eq!(map.version, FILE_VERSION);
        assert_eq!(map.title, "Title");
        assert_eq!(map.root.text, "Root");
        assert_eq!(map.node_count(), 1);
        assert!(map.validate().is_ok());
        assert!(map.root.id.starts_with("n_"));
        assert_eq!(map.root.id.len(), 10);
    }

    #[test]
    fn ids_are_unique() {
        let mut map = MindMap::new("Test", "Root");
        let root = map.root.id.clone();
        let mut ids = HashSet::new();
        ids.insert(root.clone());
        for i in 0..200 {
            let id = map.add_child(&root, format!("Child {i}")).unwrap();
            assert!(ids.insert(id), "duplicate id generated");
        }
        assert_eq!(map.node_count(), 201);
        assert!(map.validate().is_ok());
    }

    #[test]
    fn add_find_parent_and_depth() {
        let (mut map, a, _b) = map_with_children();
        assert_eq!(map.find(&a).unwrap().text, "A");
        assert_eq!(map.parent_of(&a).unwrap().id, map.root.id);
        assert_eq!(map.depth_of(&a), Some(1));
        assert_eq!(map.depth_of(&map.root.id), Some(0));
        assert!(map.parent_of(&map.root.id).is_none());

        let a2 = map.add_child(&a, "A1").unwrap();
        assert_eq!(map.depth_of(&a2), Some(2));
        assert_eq!(map.node_count(), 4);
    }

    #[test]
    fn add_child_at_clamps_index() {
        let (mut map, _a, b) = map_with_children();
        let root = map.root.id.clone();
        let inserted = map.add_child_at(&root, 1, "Between").unwrap();
        let texts: Vec<_> = map
            .find(&root)
            .unwrap()
            .children
            .iter()
            .map(|n| n.text.as_str())
            .collect();
        assert_eq!(texts, vec!["A", "Between", "B"]);

        let last = map.add_child_at(&root, 999, "Last").unwrap();
        assert!(map.find(&last).is_some());
        assert_eq!(map.find(&root).unwrap().children.len(), 4);
        assert_eq!(map.find(&b).unwrap().text, "B");
        assert_eq!(map.find(&inserted).unwrap().text, "Between");
    }

    #[test]
    fn remove_subtree() {
        let (mut map, a, _b) = map_with_children();
        let a1 = map.add_child(&a, "A1").unwrap();
        assert!(map.find(&a1).is_some());
        let removed = map.remove(&a).unwrap();
        assert_eq!(removed.id, a);
        assert_eq!(removed.children.len(), 1);
        assert!(map.find(&a).is_none());
        assert!(map.find(&a1).is_none());
        assert_eq!(map.node_count(), 2);
    }

    #[test]
    fn root_cannot_be_removed_or_moved() {
        let (mut map, a, _b) = map_with_children();
        let root = map.root.id.clone();
        assert!(matches!(map.remove(&root), Err(Error::RootRemoval)));
        assert!(matches!(map.move_node(&root, &a, 0), Err(Error::RootMove)));
    }

    #[test]
    fn move_node_reparents_and_rejects_cycles() {
        let (mut map, a, b) = map_with_children();
        let a1 = map.add_child(&a, "A1").unwrap();

        assert!(matches!(map.move_node(&a, &a1, 0), Err(Error::Cycle(_))));
        assert!(matches!(map.move_node(&a, &a, 0), Err(Error::Cycle(_))));

        map.move_node(&a1, &b, 0).unwrap();
        assert_eq!(map.find(&a).unwrap().children.len(), 0);
        assert_eq!(map.find(&b).unwrap().children.len(), 1);
        assert_eq!(map.depth_of(&a1), Some(2));
        assert!(map.find(&a).is_some());
    }

    #[test]
    fn move_node_clamps_index() {
        let mut map = MindMap::new("Test", "Root");
        let root = map.root.id.clone();
        let a = map.add_child(&root, "A").unwrap();
        let b = map.add_child(&root, "B").unwrap();
        map.move_node(&b, &root, 0).unwrap();
        let texts: Vec<_> = map
            .find(&root)
            .unwrap()
            .children
            .iter()
            .map(|n| n.text.as_str())
            .collect();
        assert_eq!(texts, vec!["B", "A"]);
        assert_eq!(map.find(&b).unwrap().text, "B");
        assert_eq!(map.find(&a).unwrap().text, "A");
    }

    #[test]
    fn insert_subtree_regenerates_ids() {
        let (mut map, a, _b) = map_with_children();
        let mut subtree = Node::new("Copy");
        let original_id = subtree.id.clone();
        let child = Node::new("Copy child");
        let child_id = child.id.clone();
        subtree.children.push(child);

        let inserted = map.insert_subtree(&a, 0, subtree).unwrap();
        assert_ne!(inserted, original_id);
        let node = map.find(&inserted).unwrap();
        assert_eq!(node.children.len(), 1);
        assert_ne!(node.children[0].id, child_id);
        assert!(map.validate().is_ok());
    }

    #[test]
    fn colors_and_positions_are_validated() {
        let (mut map, a, _b) = map_with_children();
        map.set_color(&a, Some("#ff8800")).unwrap();
        map.set_color(&a, Some("#ff8800cc")).unwrap();
        assert!(matches!(
            map.set_color(&a, Some("red")),
            Err(Error::InvalidColor(_))
        ));
        map.set_color(&a, None).unwrap();

        map.set_pos(&a, Some([10.0, -20.5])).unwrap();
        assert!(matches!(
            map.set_pos(&a, Some([f32::NAN, 0.0])),
            Err(Error::InvalidPosition)
        ));
        map.set_pos(&a, None).unwrap();
    }

    #[test]
    fn outline_is_indented_markdown() {
        let (mut map, a, _b) = map_with_children();
        map.add_child(&a, "A1\nmultiline").unwrap();
        let expected = "- Root\n  - A\n    - A1 multiline\n  - B";
        assert_eq!(map.outline(), expected);
    }

    #[test]
    fn validation_detects_duplicate_ids() {
        let mut map = MindMap::new("Test", "Root");
        let duplicate = Node {
            id: map.root.id.clone(),
            ..Node::new("Clash")
        };
        map.root.children.push(duplicate);
        assert!(matches!(map.validate(), Err(Error::DuplicateId(_))));
    }

    #[test]
    fn validation_rejects_unsupported_versions() {
        let mut map = MindMap::new("Test", "Root");
        map.version = 99;
        assert!(matches!(map.validate(), Err(Error::UnsupportedVersion(99))));
    }

    #[test]
    fn link_validation() {
        let mut map = MindMap::new("Test", "Root");
        map.links.push(Link {
            from: "n_missing".to_owned(),
            to: map.root.id.clone(),
            label: None,
        });
        assert!(matches!(map.validate(), Err(Error::InvalidLink(_, _))));
        map.links.clear();
        assert!(map.validate().is_ok());
    }

    #[test]
    fn color_helper() {
        assert!(is_valid_color("#000000"));
        assert!(is_valid_color("#aBcDeF12"));
        assert!(!is_valid_color("000000"));
        assert!(!is_valid_color("#fff"));
        assert!(!is_valid_color("#gggggg"));
    }
}
