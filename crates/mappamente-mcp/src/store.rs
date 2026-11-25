//! Filesystem-backed map storage with sandboxed names.
//!
//! Every map name is reduced to a single file name inside one directory. The
//! store never follows a path outside that directory: existing files are
//! canonicalized and checked against the canonical root, and new files are
//! joined to the root after the name has been validated.

use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use mappamente_core::io::{self, json, markdown};
use mappamente_core::{LayoutKind, MindMap};

use crate::error::StoreError;

/// Maximum length of a node label or note, in Unicode scalar values.
pub const MAX_TEXT_CHARS: usize = 10_000;

/// Maximum length of a Markdown outline accepted by the insertion tool.
pub const MAX_MARKDOWN_CHARS: usize = 100_000;

const MAP_EXTENSIONS: &[&str] = &["mappa", "json"];
const MAP_EXTENSIONS_LABEL: &str = ".mappa or .json";

/// Compact metadata for one map.
#[derive(Clone, Debug)]
pub struct MapSummary {
    /// File name inside the maps directory.
    pub name: String,
    /// Document title.
    pub title: String,
    /// Number of nodes.
    pub node_count: usize,
    /// Layout algorithm stored in the document.
    pub layout: LayoutKind,
    /// Creation timestamp in Unix milliseconds.
    pub created_at: u64,
    /// Last modification timestamp in Unix milliseconds.
    pub modified_at: u64,
}

impl MapSummary {
    fn new(name: &str, map: &MindMap) -> Self {
        Self {
            name: name.to_owned(),
            title: map.title.clone(),
            node_count: map.node_count(),
            layout: map.layout,
            created_at: map.created_at,
            modified_at: map.modified_at,
        }
    }
}

/// A map file that could not be loaded while listing.
#[derive(Clone, Debug)]
pub struct MapLoadError {
    /// File name inside the maps directory.
    pub name: String,
    /// Human-readable error message.
    pub message: String,
}

/// Result of scanning the maps directory.
#[derive(Clone, Debug, Default)]
pub struct MapListing {
    /// Loadable maps, sorted by name.
    pub maps: Vec<MapSummary>,
    /// Files with a supported extension that failed to load.
    pub errors: Vec<MapLoadError>,
}

/// The result of a mutation, including the updated document.
#[derive(Clone, Debug)]
pub struct Mutation {
    /// File name inside the maps directory.
    pub name: String,
    /// Updated document.
    pub map: MindMap,
    /// Node ids created, moved or removed by the operation.
    pub ids: Vec<String>,
}

/// Optional field updates for [`MapStore::update_node`].
#[derive(Clone, Debug, Default)]
pub struct NodeChanges<'a> {
    /// New label.
    pub text: Option<&'a str>,
    /// New note.
    pub note: Option<&'a str>,
    /// New color; an empty string clears it.
    pub color: Option<&'a str>,
    /// New collapsed state.
    pub collapsed: Option<bool>,
}

impl NodeChanges<'_> {
    fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.note.is_none()
            && self.color.is_none()
            && self.collapsed.is_none()
    }
}

/// Formats supported by [`MapStore::export`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// Standalone SVG drawing.
    Svg,
    /// Markdown outline (lossy).
    Markdown,
    /// Native `.mappa` JSON.
    Mappa,
}

impl ExportFormat {
    /// Parses a tool-level format name.
    pub fn parse(value: &str) -> Result<Self, StoreError> {
        match value.to_ascii_lowercase().as_str() {
            "svg" => Ok(Self::Svg),
            "markdown" | "md" => Ok(Self::Markdown),
            "mappa" | "json" => Ok(Self::Mappa),
            other => Err(StoreError::InvalidExportFormat(other.to_owned())),
        }
    }

    /// Canonical name used in tool responses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Markdown => "markdown",
            Self::Mappa => "mappa",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Svg => &["svg"],
            Self::Markdown => &["md", "markdown"],
            Self::Mappa => MAP_EXTENSIONS,
        }
    }

    fn default_extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Markdown => "md",
            Self::Mappa => "mappa",
        }
    }

    fn extensions_label(self) -> &'static str {
        match self {
            Self::Svg => ".svg",
            Self::Markdown => ".md or .markdown",
            Self::Mappa => MAP_EXTENSIONS_LABEL,
        }
    }
}

/// Result of writing an export file.
#[derive(Clone, Debug)]
pub struct ExportResult {
    /// File name inside the maps directory.
    pub name: String,
    /// Number of bytes written.
    pub bytes: usize,
    /// Canonical format name.
    pub format: &'static str,
}

/// A directory of `.mappa`/`.json` maps.
#[derive(Debug)]
pub struct MapStore {
    root: PathBuf,
}

impl MapStore {
    /// Opens an existing directory as the sandbox root.
    pub fn new(directory: &Path) -> Result<Self, StoreError> {
        let root = match fs::canonicalize(directory) {
            Ok(root) => root,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(StoreError::DirectoryNotFound(directory.to_path_buf()));
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        if !root.is_dir() {
            return Err(StoreError::NotADirectory(root));
        }
        Ok(Self { root })
    }

    /// Canonical sandbox root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Scans the directory for maps. Corrupt files are reported separately so
    /// one bad file does not hide the rest.
    pub fn list(&self) -> Result<MapListing, StoreError> {
        let mut listing = MapListing::default();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !is_map_file_name(&name) {
                continue;
            }
            match json::load(&entry.path()) {
                Ok(map) => listing.maps.push(MapSummary::new(&name, &map)),
                Err(error) => listing.errors.push(MapLoadError {
                    name,
                    message: error.to_string(),
                }),
            }
        }
        listing
            .maps
            .sort_by(|left, right| left.name.cmp(&right.name));
        listing
            .errors
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(listing)
    }

    /// Loads one map and returns its resolved name and path.
    pub fn load(&self, name: &str) -> Result<(String, PathBuf, MindMap), StoreError> {
        let (normalized, path) = self.resolve_with_name(name)?;
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(StoreError::MapNotFound(normalized));
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        if !metadata.is_file() {
            return Err(StoreError::NotAFile(normalized));
        }
        let map = json::load(&path)?;
        Ok((normalized, path, map))
    }

    /// Creates a new map. Existing files are never overwritten.
    pub fn create(
        &self,
        name: &str,
        title: &str,
        root_text: &str,
        layout: Option<LayoutKind>,
    ) -> Result<Mutation, StoreError> {
        check_limit("title", title, MAX_TEXT_CHARS)?;
        check_limit("root text", root_text, MAX_TEXT_CHARS)?;
        let normalized =
            normalize_simple_name(name, MAP_EXTENSIONS, Some("mappa"), MAP_EXTENSIONS_LABEL)?;
        let path = self.root.join(&normalized);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Err(StoreError::MapExists(normalized)),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(StoreError::Io(error)),
        }

        let mut map = MindMap::new(title, root_text);
        if let Some(layout) = layout {
            map.set_layout(layout);
        }
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids: Vec::new(),
        })
    }

    /// Adds a node under `parent_id`. `None` appends.
    pub fn add_node(
        &self,
        name: &str,
        parent_id: &str,
        text: &str,
        index: Option<usize>,
        expected_modified_at: Option<u64>,
    ) -> Result<Mutation, StoreError> {
        check_limit("text", text, MAX_TEXT_CHARS)?;
        let (normalized, path, mut map) = self.load(name)?;
        check_modified_at(&map, expected_modified_at)?;
        let node_id =
            map.edit(|draft| draft.add_child_at(parent_id, index.unwrap_or(usize::MAX), text))?;
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids: vec![node_id],
        })
    }

    /// Inserts a Markdown outline as children of `parent_id`.
    pub fn add_subtree_from_markdown(
        &self,
        name: &str,
        parent_id: &str,
        outline: &str,
        index: Option<usize>,
        expected_modified_at: Option<u64>,
    ) -> Result<Mutation, StoreError> {
        check_limit("markdown", outline, MAX_MARKDOWN_CHARS)?;
        let (normalized, path, mut map) = self.load(name)?;
        check_modified_at(&map, expected_modified_at)?;
        let ids = map.edit(|draft| {
            markdown::insert_markdown_children(
                draft,
                parent_id,
                index.unwrap_or(usize::MAX),
                outline,
            )
        })?;
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids,
        })
    }

    /// Applies a partial update to one node.
    pub fn update_node(
        &self,
        name: &str,
        node_id: &str,
        changes: &NodeChanges<'_>,
        expected_modified_at: Option<u64>,
    ) -> Result<Mutation, StoreError> {
        if changes.is_empty() {
            return Err(StoreError::EmptyUpdate);
        }
        if let Some(text) = changes.text {
            check_limit("text", text, MAX_TEXT_CHARS)?;
        }
        if let Some(note) = changes.note {
            check_limit("note", note, MAX_TEXT_CHARS)?;
        }
        let (normalized, path, mut map) = self.load(name)?;
        check_modified_at(&map, expected_modified_at)?;
        map.edit(|draft| {
            if let Some(text) = changes.text {
                draft.set_text(node_id, text)?;
            }
            if let Some(note) = changes.note {
                draft.set_note(node_id, note)?;
            }
            if let Some(color) = changes.color {
                let color = if color.is_empty() { None } else { Some(color) };
                draft.set_color(node_id, color)?;
            }
            if let Some(collapsed) = changes.collapsed {
                draft.set_collapsed(node_id, collapsed)?;
            }
            Ok(())
        })?;
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids: vec![node_id.to_owned()],
        })
    }

    /// Reparents a node below `new_parent_id`.
    pub fn move_node(
        &self,
        name: &str,
        node_id: &str,
        new_parent_id: &str,
        index: Option<usize>,
        expected_modified_at: Option<u64>,
    ) -> Result<Mutation, StoreError> {
        let (normalized, path, mut map) = self.load(name)?;
        check_modified_at(&map, expected_modified_at)?;
        map.edit(|draft| draft.move_node(node_id, new_parent_id, index.unwrap_or(usize::MAX)))?;
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids: vec![node_id.to_owned()],
        })
    }

    /// Removes a node and its subtree. The root is protected by the core.
    pub fn delete_node(
        &self,
        name: &str,
        node_id: &str,
        expected_modified_at: Option<u64>,
    ) -> Result<Mutation, StoreError> {
        let (normalized, path, mut map) = self.load(name)?;
        check_modified_at(&map, expected_modified_at)?;
        let removed = map.edit(|draft| draft.remove(node_id))?;
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids: vec![removed.id],
        })
    }

    /// Changes the layout algorithm stored in the document.
    pub fn set_layout(
        &self,
        name: &str,
        layout: LayoutKind,
        expected_modified_at: Option<u64>,
    ) -> Result<Mutation, StoreError> {
        let (normalized, path, mut map) = self.load(name)?;
        check_modified_at(&map, expected_modified_at)?;
        map.edit(|draft| {
            draft.set_layout(layout);
            Ok(())
        })?;
        json::save(&path, &map)?;
        Ok(Mutation {
            name: normalized,
            map,
            ids: Vec::new(),
        })
    }

    /// Writes an export file inside the maps directory.
    ///
    /// `output` is another simple file name; when omitted the export is named
    /// after the source map. Existing files require `overwrite`.
    pub fn export(
        &self,
        name: &str,
        format: ExportFormat,
        output: Option<&str>,
        overwrite: bool,
    ) -> Result<ExportResult, StoreError> {
        let (normalized, _source, map) = self.load(name)?;
        let output_name = match output {
            Some(output) => normalize_simple_name(
                output,
                format.extensions(),
                Some(format.default_extension()),
                format.extensions_label(),
            )?,
            None => default_export_name(&normalized, format),
        };
        if format == ExportFormat::Mappa && output_name.eq_ignore_ascii_case(&normalized) {
            return Err(StoreError::ExportOverwritesSource(output_name));
        }

        let output_path = self.root.join(&output_name);
        match fs::symlink_metadata(&output_path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(StoreError::InvalidName(output_name));
                }
                if !metadata.is_file() {
                    return Err(StoreError::NotAFile(output_name));
                }
                if !overwrite {
                    return Err(StoreError::ExportExists(output_name));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(StoreError::Io(error)),
        }

        let content = match format {
            ExportFormat::Svg => mappamente_core::export::svg::to_svg(&map, &Default::default()),
            ExportFormat::Markdown => markdown::to_markdown(&map),
            ExportFormat::Mappa => json::to_string(&map)?,
        };
        json::write_atomic(&output_path, content.as_bytes())?;
        Ok(ExportResult {
            name: output_name,
            bytes: content.len(),
            format: format.as_str(),
        })
    }

    /// Validates the name and returns the normalized name and safe path.
    fn resolve_with_name(&self, name: &str) -> Result<(String, PathBuf), StoreError> {
        let normalized =
            normalize_simple_name(name, MAP_EXTENSIONS, Some("mappa"), MAP_EXTENSIONS_LABEL)?;
        let candidate = self.root.join(&normalized);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {
                let canonical = fs::canonicalize(&candidate)?;
                if !canonical.starts_with(&self.root) {
                    return Err(StoreError::InvalidName(normalized));
                }
                Ok((normalized, canonical))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok((normalized, candidate)),
            Err(error) => Err(StoreError::Io(error)),
        }
    }
}

/// Parses a layout name from a tool argument.
pub fn parse_layout(value: &str) -> Result<LayoutKind, StoreError> {
    match value.to_ascii_lowercase().as_str() {
        "tree" | "bilateral" => Ok(LayoutKind::Tree),
        "radial" => Ok(LayoutKind::Radial),
        "logical" => Ok(LayoutKind::Logical),
        other => Err(StoreError::InvalidLayout(other.to_owned())),
    }
}

fn check_modified_at(map: &MindMap, expected: Option<u64>) -> Result<(), StoreError> {
    if let Some(expected) = expected
        && map.modified_at != expected
    {
        return Err(StoreError::ModifiedAtMismatch {
            expected,
            actual: map.modified_at,
        });
    }
    Ok(())
}

fn check_limit(field: &'static str, value: &str, limit: usize) -> Result<(), StoreError> {
    let actual = value.chars().count();
    if actual > limit {
        return Err(StoreError::LimitExceeded {
            field,
            limit,
            actual,
        });
    }
    Ok(())
}

fn is_map_file_name(name: &str) -> bool {
    matches!(
        io::extension(Path::new(name)).as_deref(),
        Some("mappa" | "json")
    )
}

fn default_export_name(map_name: &str, format: ExportFormat) -> String {
    let stem = Path::new(map_name)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("map");
    match format {
        ExportFormat::Svg => format!("{stem}.svg"),
        ExportFormat::Markdown => format!("{stem}.md"),
        ExportFormat::Mappa => format!("{stem}.export.mappa"),
    }
}

/// Validates a simple file name and fills in `default_extension` when the name
/// has none. `allowed` contains lowercase extensions without the dot.
fn normalize_simple_name(
    name: &str,
    allowed: &[&str],
    default_extension: Option<&str>,
    allowed_label: &'static str,
) -> Result<String, StoreError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.chars().any(|ch| matches!(ch, '/' | '\\' | ':' | '\0'))
        || is_reserved_windows_name(name)
    {
        return Err(StoreError::InvalidName(name.to_owned()));
    }

    let path = Path::new(name);
    let mut components = path.components();
    let single_component = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(part)), None) if part == OsStr::new(name)
    );
    if !single_component {
        return Err(StoreError::InvalidName(name.to_owned()));
    }

    match path.extension().and_then(OsStr::to_str) {
        None => match default_extension {
            Some(extension) => Ok(format!("{name}.{extension}")),
            None => Err(StoreError::UnsupportedExtension {
                name: name.to_owned(),
                allowed: allowed_label,
            }),
        },
        Some(extension)
            if allowed
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed)) =>
        {
            Ok(name.to_owned())
        }
        Some(_) => Err(StoreError::UnsupportedExtension {
            name: name.to_owned(),
            allowed: allowed_label,
        }),
    }
}

/// Windows strips trailing dots/spaces and reserves device names. Rejecting
/// them keeps a name pointing at the same file on every platform.
fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let stem = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9')
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use mappamente_core::Error as CoreError;

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mappamente-mcp-{label}-{}-{}",
                std::process::id(),
                fastrand::u64(..)
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn store(label: &str) -> (TempDir, MapStore) {
        let directory = TempDir::new(label);
        let store = MapStore::new(directory.path()).unwrap();
        (directory, store)
    }

    #[test]
    fn new_requires_an_existing_directory() {
        let directory = TempDir::new("missing");
        let missing = directory.path().join("absent");
        assert!(matches!(
            MapStore::new(&missing),
            Err(StoreError::DirectoryNotFound(_))
        ));

        let file = directory.path().join("plain.txt");
        fs::write(&file, "not a directory").unwrap();
        assert!(matches!(
            MapStore::new(&file),
            Err(StoreError::NotADirectory(_))
        ));
    }

    #[test]
    fn names_are_sandboxed_to_simple_files() {
        let (_directory, store) = store("names");
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "sub/map.mappa",
            "sub\\map.mappa",
            "C:\\temp\\map.mappa",
            "map.txt",
            "map.mappa.",
            "map ",
        ] {
            assert!(store.load(name).is_err(), "accepted {name:?}");
        }

        assert!(matches!(
            store.load("CON.mappa"),
            Err(StoreError::InvalidName(_))
        ));
    }

    #[test]
    fn existing_symlinks_outside_the_root_are_rejected() {
        let (_directory, store) = store("symlink");
        let outside = TempDir::new("outside");
        let target = outside.path().join("secret.mappa");
        json::save(&target, &MindMap::new("Secret", "Root")).unwrap();
        let link = store.root().join("linked.mappa");

        #[cfg(unix)]
        let created = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        #[cfg(not(any(unix, windows)))]
        let created = false;

        if !created {
            // Windows without Developer Mode cannot create symlinks.
            return;
        }

        assert!(matches!(
            store.load("linked"),
            Err(StoreError::InvalidName(_))
        ));
    }

    #[test]
    fn create_load_list_and_extend() {
        let (_directory, store) = store("roundtrip");
        let summary = store
            .create("ideas", "Ideas", "Launch", Some(LayoutKind::Tree))
            .unwrap();
        assert_eq!(summary.name, "ideas.mappa");
        assert_eq!(summary.map.title, "Ideas");
        assert_eq!(summary.map.node_count(), 1);
        assert_eq!(summary.map.layout, LayoutKind::Tree);
        assert!(store.create("ideas", "Again", "Root", None).is_err());

        let (name, path, map) = store.load("ideas.mappa").unwrap();
        assert_eq!(name, "ideas.mappa");
        assert!(path.starts_with(store.root()));
        assert_eq!(map.title, "Ideas");

        let mutation = store
            .add_node("ideas", &map.root.id, "First", None, None)
            .unwrap();
        assert_eq!(mutation.ids.len(), 1);
        assert_eq!(mutation.map.node_count(), 2);

        let listing = store.list().unwrap();
        assert_eq!(listing.maps.len(), 1);
        assert_eq!(listing.maps[0].name, "ideas.mappa");
        assert!(listing.errors.is_empty());
    }

    #[test]
    fn list_reports_corrupt_maps_without_hiding_valid_ones() {
        let (_directory, store) = store("corrupt");
        store.create("good", "Good", "Root", None).unwrap();
        fs::write(store.root().join("broken.mappa"), "not json").unwrap();
        fs::write(store.root().join("ignored.txt"), "text").unwrap();

        let listing = store.list().unwrap();
        assert_eq!(listing.maps.len(), 1);
        assert_eq!(listing.maps[0].name, "good.mappa");
        assert_eq!(listing.errors.len(), 1);
        assert_eq!(listing.errors[0].name, "broken.mappa");
    }

    #[test]
    fn update_move_and_delete_keep_invariants() {
        let (_directory, store) = store("mutations");
        store.create("map", "Map", "Root", None).unwrap();
        let (_, _, map) = store.load("map").unwrap();
        let root = map.root.id.clone();
        let first = store
            .add_node("map", &root, "First", None, None)
            .unwrap()
            .ids[0]
            .clone();
        let second = store
            .add_node("map", &root, "Second", None, None)
            .unwrap()
            .ids[0]
            .clone();
        let child = store
            .add_node("map", &first, "Child", None, None)
            .unwrap()
            .ids[0]
            .clone();

        let mutated = store
            .update_node(
                "map",
                &first,
                &NodeChanges {
                    text: Some("Renamed"),
                    note: Some("A note"),
                    color: Some("#ff8800"),
                    collapsed: Some(true),
                },
                None,
            )
            .unwrap();
        let node = mutated.map.find(&first).unwrap();
        assert_eq!(node.text, "Renamed");
        assert_eq!(node.note, "A note");
        assert_eq!(node.color.as_deref(), Some("#ff8800"));
        assert!(node.collapsed);
        assert_eq!(mutated.map.node_count(), 4);

        let cleared = store
            .update_node(
                "map",
                &first,
                &NodeChanges {
                    color: Some(""),
                    ..NodeChanges::default()
                },
                None,
            )
            .unwrap();
        assert!(cleared.map.find(&first).unwrap().color.is_none());

        let moved = store
            .move_node("map", &second, &first, Some(0), None)
            .unwrap();
        assert_eq!(moved.map.parent_of(&second).unwrap().id, first);
        let first_children = &moved.map.find(&first).unwrap().children;
        assert_eq!(first_children.len(), 2);
        assert_eq!(first_children[0].id, second);
        assert_eq!(first_children[1].id, child);

        let deleted = store.delete_node("map", &first, None).unwrap();
        assert_eq!(deleted.ids, vec![first.clone()]);
        assert!(deleted.map.find(&first).is_none());
        assert!(deleted.map.find(&child).is_none());
        assert!(deleted.map.find(&second).is_none());
        assert_eq!(deleted.map.node_count(), 1);

        assert!(matches!(
            store.delete_node("map", &root, None),
            Err(StoreError::Core(CoreError::RootRemoval))
        ));
    }

    #[test]
    fn markdown_insertion_creates_ids_and_keeps_atomicity() {
        let (_directory, store) = store("markdown");
        store.create("map", "Map", "Root", None).unwrap();
        let (_, _, map) = store.load("map").unwrap();
        let root = map.root.id.clone();

        let mutation = store
            .add_subtree_from_markdown(
                "map",
                &root,
                "- Outline\n  - Alpha\n    - Detail\n  - Beta\n",
                None,
                None,
            )
            .unwrap();
        assert_eq!(mutation.ids.len(), 2);
        assert_eq!(mutation.map.node_count(), 4);
        assert_eq!(mutation.map.find(&root).unwrap().children[0].text, "Alpha");

        let before = store.load("map").unwrap().2;
        assert!(matches!(
            store.add_subtree_from_markdown("map", "n_missing", "- A\n", None, None),
            Err(StoreError::Core(CoreError::NodeNotFound(_)))
        ));
        assert_eq!(store.load("map").unwrap().2, before);
    }

    #[test]
    fn optimistic_concurrency_rejects_a_stale_token() {
        let (_directory, store) = store("concurrency");
        store.create("map", "Map", "Root", None).unwrap();
        let (_, _, map) = store.load("map").unwrap();
        let root = map.root.id.clone();
        let stale = map.modified_at;

        let mutation = store
            .add_node("map", &root, "Child", None, Some(stale))
            .unwrap();
        assert!(mutation.map.modified_at > stale);
        let current = mutation.map.modified_at;

        let error = store
            .add_node("map", &root, "Other", None, Some(stale))
            .unwrap_err();
        assert!(matches!(
            error,
            StoreError::ModifiedAtMismatch {
                expected,
                actual
            } if expected == stale && actual == current
        ));

        let mutation = store
            .add_node("map", &root, "Other", None, Some(current))
            .unwrap();
        assert_eq!(mutation.map.node_count(), 3);
    }

    #[test]
    fn limits_are_enforced_before_mutating() {
        let (_directory, store) = store("limits");
        store.create("map", "Map", "Root", None).unwrap();
        let (_, _, map) = store.load("map").unwrap();
        let root = map.root.id.clone();

        let long_text = "x".repeat(MAX_TEXT_CHARS + 1);
        let error = store
            .add_node("map", &root, &long_text, None, None)
            .unwrap_err();
        assert!(matches!(
            error,
            StoreError::LimitExceeded {
                field: "text",
                limit: MAX_TEXT_CHARS,
                ..
            }
        ));

        let long_markdown = "- x\n".repeat(MAX_MARKDOWN_CHARS);
        let error = store
            .add_subtree_from_markdown("map", &root, &long_markdown, None, None)
            .unwrap_err();
        assert!(matches!(
            error,
            StoreError::LimitExceeded {
                field: "markdown",
                limit: MAX_MARKDOWN_CHARS,
                ..
            }
        ));

        assert_eq!(store.load("map").unwrap().2.node_count(), 1);
        assert!(matches!(
            store.update_node("map", &root, &NodeChanges::default(), None),
            Err(StoreError::EmptyUpdate)
        ));
    }

    #[test]
    fn export_writes_inside_the_directory_and_never_overwrites_the_source() {
        let (_directory, store) = store("export");
        store.create("map", "Map", "Root", None).unwrap();

        let svg = store.export("map", ExportFormat::Svg, None, false).unwrap();
        assert_eq!(svg.name, "map.svg");
        assert_eq!(svg.format, "svg");
        let text = fs::read_to_string(store.root().join("map.svg")).unwrap();
        assert!(text.starts_with("<?xml"));
        assert!(matches!(
            store.export("map", ExportFormat::Svg, None, false),
            Err(StoreError::ExportExists(_))
        ));
        store.export("map", ExportFormat::Svg, None, true).unwrap();

        let markdown = store
            .export("map", ExportFormat::Markdown, Some("outline"), false)
            .unwrap();
        assert_eq!(markdown.name, "outline.md");
        let text = fs::read_to_string(store.root().join("outline.md")).unwrap();
        assert!(text.starts_with("# Map"));

        let native = store
            .export("map", ExportFormat::Mappa, None, false)
            .unwrap();
        assert_eq!(native.name, "map.export.mappa");
        assert!(matches!(
            store.export("map", ExportFormat::Mappa, Some("map.mappa"), true),
            Err(StoreError::ExportOverwritesSource(_))
        ));
        assert!(matches!(
            store.export("map", ExportFormat::Svg, Some("../escape.svg"), false),
            Err(StoreError::InvalidName(_)) | Err(StoreError::UnsupportedExtension { .. })
        ));
    }

    #[test]
    fn layouts_accept_aliases_and_reject_unknown_names() {
        assert_eq!(parse_layout("tree").unwrap(), LayoutKind::Tree);
        assert_eq!(parse_layout("BILATERAL").unwrap(), LayoutKind::Tree);
        assert_eq!(parse_layout("Radial").unwrap(), LayoutKind::Radial);
        assert_eq!(parse_layout("logical").unwrap(), LayoutKind::Logical);
        assert!(matches!(
            parse_layout("circular"),
            Err(StoreError::InvalidLayout(_))
        ));
    }
}
