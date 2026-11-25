//! rmcp stdio server exposing the map store as MCP tools.

use std::sync::Arc;

use mappamente_core::{LayoutKind, MindMap};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::StoreError;
use crate::store::{ExportFormat, MapStore, MapSummary, Mutation, NodeChanges, parse_layout};

/// MCP server backed by one maps directory.
#[derive(Clone)]
pub struct MappamenteServer {
    store: Arc<MapStore>,
    tool_router: ToolRouter<Self>,
}

impl MappamenteServer {
    /// Builds the server and its tool router.
    #[must_use]
    pub fn new(store: MapStore) -> Self {
        Self {
            store: Arc::new(store),
            tool_router: Self::tool_router(),
        }
    }

    fn respond<T>(
        &self,
        result: Result<T, StoreError>,
        render: impl FnOnce(T) -> Value,
    ) -> Result<CallToolResult, McpError> {
        match result {
            Ok(value) => Ok(CallToolResult::structured(render(value))),
            Err(error) => {
                eprintln!("mappamente-mcp: tool error: {error}");
                Ok(CallToolResult::structured_error(json!({
                    "error": {
                        "code": error.code(),
                        "message": error.to_string(),
                    }
                })))
            }
        }
    }
}

#[tool_router]
impl MappamenteServer {
    #[tool(
        description = "List the .mappa and .json maps in the server directory. Returns name, title, node count, layout and modification timestamp for each map; unreadable maps are reported in a separate `errors` array."
    )]
    fn list_maps(&self) -> Result<CallToolResult, McpError> {
        self.respond(self.store.list(), |listing| {
            let maps: Vec<Value> = listing.maps.iter().map(summary_json).collect();
            let errors: Vec<Value> = listing
                .errors
                .iter()
                .map(|error| {
                    json!({
                        "name": error.name,
                        "message": error.message,
                    })
                })
                .collect();
            json!({
                "directory": self.store.root().display().to_string(),
                "maps": maps,
                "errors": errors,
            })
        })
    }

    #[tool(
        description = "Return one map with its metadata and an indented outline. Every outline line starts with the node id in brackets, e.g. `- [n_ab12cd34] Root`; use those ids as node_id/parent_id in the other tools. Notes and colors appear inline when set."
    )]
    fn get_map(
        &self,
        Parameters(params): Parameters<GetMapParams>,
    ) -> Result<CallToolResult, McpError> {
        self.respond(self.store.load(&params.map), |(name, _path, map)| {
            document_json(&name, &map)
        })
    }

    #[tool(
        description = "Create a new map in the server directory. `map` is a simple file name and may omit the extension (defaults to .mappa). Existing files are never overwritten. Optional layout: radial, logical or tree."
    )]
    fn create_map(
        &self,
        Parameters(params): Parameters<CreateMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = (|| {
            let layout = params.layout.as_deref().map(parse_layout).transpose()?;
            self.store
                .create(&params.map, &params.title, &params.root, layout)
        })();
        self.respond(result, |mutation| mutation_json(&mutation))
    }

    #[tool(
        description = "Add a child node below parent_id and return the new node id. `index` is clamped; omit it to append. Text is limited to 10,000 characters. Pass expected_modified_at from the latest read to reject concurrent edits."
    )]
    fn add_node(
        &self,
        Parameters(params): Parameters<AddNodeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.respond(
            self.store.add_node(
                &params.map,
                &params.parent_id,
                &params.text,
                params.index,
                params.expected_modified_at,
            ),
            |mutation| mutation_json(&mutation),
        )
    }

    #[tool(
        description = "Insert a Markdown outline as children of parent_id. When the parsed outline has one root with children, the root is treated as a heading and discarded; a single-line outline inserts that node. Markdown is limited to 100,000 characters. All ids are regenerated. Pass expected_modified_at to reject concurrent edits."
    )]
    fn add_subtree_from_markdown(
        &self,
        Parameters(params): Parameters<AddSubtreeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.respond(
            self.store.add_subtree_from_markdown(
                &params.map,
                &params.parent_id,
                &params.markdown,
                params.index,
                params.expected_modified_at,
            ),
            |mutation| mutation_json(&mutation),
        )
    }

    #[tool(
        description = "Update one node. Supply at least one of text, note, color or collapsed. Color must be #RRGGBB or #RRGGBBAA; an empty string clears it. Text and notes are limited to 10,000 characters. Pass expected_modified_at to reject concurrent edits."
    )]
    fn update_node(
        &self,
        Parameters(params): Parameters<UpdateNodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let changes = NodeChanges {
            text: params.text.as_deref(),
            note: params.note.as_deref(),
            color: params.color.as_deref(),
            collapsed: params.collapsed,
        };
        self.respond(
            self.store.update_node(
                &params.map,
                &params.node_id,
                &changes,
                params.expected_modified_at,
            ),
            |mutation| mutation_json(&mutation),
        )
    }

    #[tool(
        description = "Reparent node_id below new_parent_id at an optional index (clamped; omitted appends). Moving a node inside its own subtree is rejected. The root cannot be moved. Pass expected_modified_at to reject concurrent edits."
    )]
    fn move_node(
        &self,
        Parameters(params): Parameters<MoveNodeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.respond(
            self.store.move_node(
                &params.map,
                &params.node_id,
                &params.new_parent_id,
                params.index,
                params.expected_modified_at,
            ),
            |mutation| mutation_json(&mutation),
        )
    }

    #[tool(
        description = "Delete node_id and its whole subtree. The root cannot be deleted. The removed id is returned in node_ids. Pass expected_modified_at to reject concurrent edits."
    )]
    fn delete_node(
        &self,
        Parameters(params): Parameters<DeleteNodeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.respond(
            self.store
                .delete_node(&params.map, &params.node_id, params.expected_modified_at),
            |mutation| mutation_json(&mutation),
        )
    }

    #[tool(
        description = "Set the map layout to radial, logical or tree (bilateral). Pass expected_modified_at to reject concurrent edits."
    )]
    fn set_layout(
        &self,
        Parameters(params): Parameters<SetLayoutParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = (|| {
            let layout = parse_layout(&params.layout)?;
            self.store
                .set_layout(&params.map, layout, params.expected_modified_at)
        })();
        self.respond(result, |mutation| mutation_json(&mutation))
    }

    #[tool(
        description = "Export a map inside the server directory as svg, markdown or mappa (native JSON). `output` is a simple file name and may omit the extension. Existing files require overwrite=true. A native export never overwrites its source map."
    )]
    fn export(
        &self,
        Parameters(params): Parameters<ExportParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = (|| {
            let format = ExportFormat::parse(&params.format)?;
            self.store.export(
                &params.map,
                format,
                params.output.as_deref(),
                params.overwrite,
            )
        })();
        self.respond(result, |export| {
            json!({
                "exported": export.name,
                "bytes": export.bytes,
                "format": export.format,
            })
        })
    }
}

#[tool_handler(router = self.tool_router.clone())]
impl ServerHandler for MappamenteServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "mappamente-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Mappamente mind maps over stdio. Call list_maps first, then get_map \
                 to obtain node ids. Pass expected_modified_at from the latest response \
                 on every mutation to detect concurrent edits. All files stay inside \
                 the directory given with --dir.",
            )
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetMapParams {
    /// Map file name, e.g. "ideas" or "ideas.mappa".
    map: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateMapParams {
    /// Map file name, e.g. "ideas" or "ideas.mappa".
    map: String,
    /// Document title.
    #[serde(default = "default_title")]
    title: String,
    /// Text of the root node.
    #[serde(default = "default_root")]
    root: String,
    /// Optional layout: radial, logical or tree.
    #[serde(default)]
    layout: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddNodeParams {
    /// Map file name.
    map: String,
    /// Existing node that receives the new child.
    parent_id: String,
    /// New node label (maximum 10,000 characters).
    text: String,
    /// Child position; omitted appends.
    #[serde(default)]
    index: Option<usize>,
    /// Value of modified_at from the latest read, for optimistic concurrency.
    #[serde(default)]
    expected_modified_at: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddSubtreeParams {
    /// Map file name.
    map: String,
    /// Existing node that receives the new children.
    parent_id: String,
    /// Markdown outline with a single root bullet (maximum 100,000 characters).
    markdown: String,
    /// Child position; omitted appends.
    #[serde(default)]
    index: Option<usize>,
    /// Value of modified_at from the latest read, for optimistic concurrency.
    #[serde(default)]
    expected_modified_at: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateNodeParams {
    /// Map file name.
    map: String,
    /// Node to update.
    node_id: String,
    /// New label (maximum 10,000 characters).
    #[serde(default)]
    text: Option<String>,
    /// New note (maximum 10,000 characters).
    #[serde(default)]
    note: Option<String>,
    /// New color (#RRGGBB or #RRGGBBAA); an empty string clears it.
    #[serde(default)]
    color: Option<String>,
    /// New collapsed state.
    #[serde(default)]
    collapsed: Option<bool>,
    /// Value of modified_at from the latest read, for optimistic concurrency.
    #[serde(default)]
    expected_modified_at: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MoveNodeParams {
    /// Map file name.
    map: String,
    /// Node to move.
    node_id: String,
    /// New parent node.
    new_parent_id: String,
    /// Position below the new parent; omitted appends.
    #[serde(default)]
    index: Option<usize>,
    /// Value of modified_at from the latest read, for optimistic concurrency.
    #[serde(default)]
    expected_modified_at: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteNodeParams {
    /// Map file name.
    map: String,
    /// Node to remove together with its subtree.
    node_id: String,
    /// Value of modified_at from the latest read, for optimistic concurrency.
    #[serde(default)]
    expected_modified_at: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetLayoutParams {
    /// Map file name.
    map: String,
    /// Layout name: radial, logical or tree.
    layout: String,
    /// Value of modified_at from the latest read, for optimistic concurrency.
    #[serde(default)]
    expected_modified_at: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExportParams {
    /// Map file name.
    map: String,
    /// Export format: svg, markdown or mappa.
    format: String,
    /// Output file name; omitted derives it from the map name.
    #[serde(default)]
    output: Option<String>,
    /// Replace an existing output file.
    #[serde(default)]
    overwrite: bool,
}

fn default_title() -> String {
    "Untitled map".to_owned()
}

fn default_root() -> String {
    "Central idea".to_owned()
}

fn summary_json(summary: &MapSummary) -> Value {
    json!({
        "name": summary.name,
        "title": summary.title,
        "node_count": summary.node_count,
        "layout": layout_name(summary.layout),
        "created_at": summary.created_at,
        "modified_at": summary.modified_at,
    })
}

fn summary_json_from_map(name: &str, map: &MindMap) -> Value {
    json!({
        "name": name,
        "title": map.title,
        "node_count": map.node_count(),
        "layout": layout_name(map.layout),
        "created_at": map.created_at,
        "modified_at": map.modified_at,
    })
}

fn document_json(name: &str, map: &MindMap) -> Value {
    json!({
        "name": name,
        "title": map.title,
        "version": map.version,
        "theme": map.theme,
        "layout": layout_name(map.layout),
        "created_at": map.created_at,
        "modified_at": map.modified_at,
        "node_count": map.node_count(),
        "root_id": map.root.id,
        "outline": map.outline_with_ids(),
    })
}

fn mutation_json(mutation: &Mutation) -> Value {
    json!({
        "map": summary_json_from_map(&mutation.name, &mutation.map),
        "modified_at": mutation.map.modified_at,
        "node_count": mutation.map.node_count(),
        "outline": mutation.map.outline_with_ids(),
        "node_ids": mutation.ids,
    })
}

fn layout_name(layout: LayoutKind) -> &'static str {
    match layout {
        LayoutKind::Tree => "tree",
        LayoutKind::Radial => "radial",
        LayoutKind::Logical => "logical",
    }
}
