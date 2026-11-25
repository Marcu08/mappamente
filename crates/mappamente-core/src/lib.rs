//! Core of **Mappamente**, a lightweight open-source mind map editor.
//!
//! This crate contains everything that does not depend on a GUI:
//!
//! - the [`MindMap`] document model and its invariants ([`model`]);
//! - a deterministic layout engine for radial, logical and bilateral trees
//!   ([`layout`]);
//! - undo/redo with transactions and coalescing ([`history`]);
//! - the native `.mappa` JSON format and atomic persistence ([`io`]).
//!
//! Coordinates use a y-down canvas space. Units are logical pixels.

#![forbid(unsafe_code)]

pub mod error;
pub mod export;
pub mod geom;
pub mod history;
pub mod io;
pub mod layout;
pub mod model;
pub mod theme;

pub use error::{Error, Result};
pub use geom::{Point, Rect, Size};
pub use history::Editor;
pub use layout::{Layout, LayoutNode, LayoutOptions, Metrics, Side};
pub use model::{FILE_EXTENSION, FILE_VERSION, LayoutKind, Link, MindMap, Node, NodeId};
pub use theme::Palette;
