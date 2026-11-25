//! AI provider clients and the JSON editing protocol used by Mappamente.
//!
//! The crate is deliberately independent of egui and of the MCP server. The
//! GUI snapshots the live [`MindMap`](mappamente_core::MindMap), sends an
//! outline to a provider, and applies the returned actions inside one undoable
//! `Editor` transaction.

#![forbid(unsafe_code)]

pub mod action;
pub mod client;
pub mod credential;
pub mod error;
pub mod prompt;
pub mod provider;

pub use action::{
    ActionNode, ApplySummary, MAX_MARKDOWN_CHARS, MAX_TEXT_CHARS, MapAction, Plan, apply_actions,
    parse_plan,
};
pub use client::{ChatMessage, ChatRequest, ChatResponse, ProviderClient, Role, TokenUsage};
pub use credential::{KeySource, clear_api_key, key_source, load_api_key, save_api_key};
pub use error::{AiError, Result};
pub use prompt::{MAX_CONTEXT_CHARS, map_context, system_prompt};
pub use provider::{PRESETS, Protocol, ProviderPreset, ProviderSettings, preset_by_id};
