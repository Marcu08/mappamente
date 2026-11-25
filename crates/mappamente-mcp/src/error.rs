//! Errors returned by the stdio MCP server.
//!
//! Every variant is a domain error that the tools report as a tool-level
//! `isError` result; none of them escapes as a panic or a protocol error.

use std::path::PathBuf;

use mappamente_core::Error as CoreError;
use thiserror::Error;

/// Everything that can go wrong while serving map files.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The `--dir` directory does not exist.
    #[error("maps directory {0} does not exist")]
    DirectoryNotFound(PathBuf),

    /// The `--dir` path is not a directory.
    #[error("{0} is not a directory")]
    NotADirectory(PathBuf),

    /// A name is not a simple file name inside the maps directory.
    #[error("invalid map name {0:?}: use a simple file name without directories")]
    InvalidName(String),

    /// A name uses an extension that the operation does not support.
    #[error("unsupported extension for {name:?}: expected {allowed}")]
    UnsupportedExtension {
        /// The rejected name.
        name: String,
        /// Human-readable list of accepted extensions.
        allowed: &'static str,
    },

    /// The requested map file does not exist.
    #[error("map {0:?} was not found")]
    MapNotFound(String),

    /// Creating a map would overwrite an existing file.
    #[error("map {0:?} already exists")]
    MapExists(String),

    /// Exporting would overwrite an existing file and `overwrite` was false.
    #[error("export file {0:?} already exists; pass overwrite=true to replace it")]
    ExportExists(String),

    /// The path exists but is not a regular file.
    #[error("{0:?} is not a regular file")]
    NotAFile(String),

    /// The caller's optimistic-concurrency token is stale.
    #[error("modified_at mismatch: expected {expected}, found {actual}; reload the map and retry")]
    ModifiedAtMismatch {
        /// Value supplied by the caller.
        expected: u64,
        /// Value stored in the map.
        actual: u64,
    },

    /// A text input exceeds its defensive limit.
    #[error("{field} is too long: {actual} characters, maximum {limit}")]
    LimitExceeded {
        /// Field name used in the message.
        field: &'static str,
        /// Maximum number of characters.
        limit: usize,
        /// Actual number of characters.
        actual: usize,
    },

    /// `update_node` was called without any field to update.
    #[error("no update fields were supplied")]
    EmptyUpdate,

    /// A layout name is not one of the supported algorithms.
    #[error("unsupported layout {0:?}: expected radial, logical or tree")]
    InvalidLayout(String),

    /// An export format name is not supported.
    #[error("unsupported export format {0:?}: expected svg, markdown or mappa")]
    InvalidExportFormat(String),

    /// A native export would overwrite the source map.
    #[error("cannot export {0:?} over its source map")]
    ExportOverwritesSource(String),

    /// A core model or persistence error.
    #[error(transparent)]
    Core(#[from] CoreError),

    /// A filesystem error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StoreError {
    /// Stable machine-readable code included in tool error results.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::DirectoryNotFound(_) => "directory_not_found",
            Self::NotADirectory(_) => "not_a_directory",
            Self::InvalidName(_) => "invalid_map_name",
            Self::UnsupportedExtension { .. } => "unsupported_extension",
            Self::MapNotFound(_) => "map_not_found",
            Self::MapExists(_) => "map_exists",
            Self::ExportExists(_) => "export_exists",
            Self::NotAFile(_) => "not_a_file",
            Self::ModifiedAtMismatch { .. } => "modified_at_mismatch",
            Self::LimitExceeded { .. } => "limit_exceeded",
            Self::EmptyUpdate => "empty_update",
            Self::InvalidLayout(_) => "invalid_layout",
            Self::InvalidExportFormat(_) => "invalid_export_format",
            Self::ExportOverwritesSource(_) => "export_overwrites_source",
            Self::Core(error) => match error {
                CoreError::NodeNotFound(_) => "node_not_found",
                CoreError::RootRemoval | CoreError::RootMove => "root_protected",
                CoreError::Cycle(_) => "cycle",
                CoreError::InvalidColor(_) => "invalid_color",
                CoreError::FileTooLarge { .. } => "map_too_large",
                CoreError::UnsupportedVersion(_) => "unsupported_version",
                CoreError::Parse(_) | CoreError::Json(_) => "invalid_map",
                CoreError::Io(_) => "io_error",
                _ => "invalid_map",
            },
            Self::Io(_) => "io_error",
        }
    }
}
