//! stdio [Model Context Protocol](https://modelcontextprotocol.io) server for
//! Mappamente.
//!
//! The server exposes a directory of `.mappa`/`.json` maps as MCP tools. Every
//! tool operates on simple file names inside the directory given with `--dir`;
//! no path outside that directory can be read or written. Mutations load the
//! map, apply a transactional [`MindMap::edit`](mappamente_core::MindMap::edit),
//! and write the result with the core atomic writer.

#![forbid(unsafe_code)]

pub mod error;
pub mod server;
pub mod store;

pub use error::StoreError;
pub use server::MappamenteServer;
pub use store::{
    ExportFormat, ExportResult, MAX_MARKDOWN_CHARS, MAX_TEXT_CHARS, MapListing, MapLoadError,
    MapStore, MapSummary, Mutation, NodeChanges, parse_layout,
};
