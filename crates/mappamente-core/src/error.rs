//! Errors returned by the core crate.

use crate::model::NodeId;

/// Result alias used throughout `mappamente-core`.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while editing or persisting a map.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The requested node does not exist in the map.
    #[error("node not found: {0}")]
    NodeNotFound(NodeId),

    /// The root node is the map itself and cannot be removed.
    #[error("the root node cannot be removed")]
    RootRemoval,

    /// The root node has no parent and cannot be reparented.
    #[error("the root node cannot be moved")]
    RootMove,

    /// A move would detach a node and attach it below itself.
    #[error("cannot move node {0} inside its own subtree")]
    Cycle(NodeId),

    /// A color is not in `#RRGGBB` or `#RRGGBBAA` form.
    #[error("invalid color {0:?}: expected #RRGGBB or #RRGGBBAA")]
    InvalidColor(String),

    /// A manual position contains NaN or infinite coordinates.
    #[error("invalid position: coordinates must be finite")]
    InvalidPosition,

    /// Two nodes share the same id.
    #[error("duplicate node id: {0}")]
    DuplicateId(NodeId),

    /// A node id is empty, too long or contains unsupported characters.
    #[error("invalid node id: {0:?}")]
    InvalidId(String),

    /// A cross-link references a missing node.
    #[error("invalid link {0:?} -> {1:?}: both nodes must exist")]
    InvalidLink(NodeId, NodeId),

    /// A file exceeds the defensive size limit.
    #[error("map file is too large ({size} bytes); maximum is {limit} bytes")]
    FileTooLarge { size: u64, limit: u64 },

    /// A text input could not be parsed into a map.
    #[error("{0}")]
    Parse(String),

    /// The file was written by a newer, unsupported version of Mappamente.
    #[error("unsupported file version {0}; this build supports version 1")]
    UnsupportedVersion(u32),

    /// The JSON text could not be parsed into a map.
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// Reading or writing a file failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}
