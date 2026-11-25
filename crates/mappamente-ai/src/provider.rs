//! Built-in AI provider presets.
//!
//! The presets are starting points: the GUI lets the user change the model and
//! base URL. Cloud providers use an API key; Ollama and LM Studio run locally
//! and do not require one.

/// How a provider expects chat requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// OpenAI chat-completions shape (`/chat/completions`).
    OpenAiCompatible,
    /// Anthropic Messages shape (`/v1/messages`).
    Anthropic,
    /// OpenCode server one-shot generation (`/api/experimental/generate/text`).
    OpenCodeServer,
}

/// A built-in provider configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderPreset {
    /// Stable id used for the keyring entry.
    pub id: &'static str,
    /// Human-readable name shown in the UI.
    pub name: &'static str,
    /// Request/response protocol.
    pub protocol: Protocol,
    /// Default base URL.
    pub base_url: &'static str,
    /// Default model id; editable in the UI.
    pub model: &'static str,
    /// Environment variable used as a key fallback.
    pub key_env: &'static str,
    /// Whether the provider needs an API key.
    pub needs_key: bool,
    /// Short note shown under the provider selector.
    pub note: &'static str,
}

/// Providers shown in the selector, in display order.
pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "openai",
        name: "OpenAI",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.openai.com/v1",
        model: "gpt-5.2",
        key_env: "OPENAI_API_KEY",
        needs_key: true,
        note: "OpenAI API, OpenAI-compatible chat completions.",
    },
    ProviderPreset {
        id: "anthropic",
        name: "Anthropic",
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com",
        model: "claude-sonnet-4-5",
        key_env: "ANTHROPIC_API_KEY",
        needs_key: true,
        note: "Anthropic Messages API.",
    },
    ProviderPreset {
        id: "opencode",
        name: "OpenCode Console",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://opencode.ai/inference/openai/v1",
        model: "deepseek-v4.1-flash",
        key_env: "OPENCODE_API_KEY",
        needs_key: true,
        note: "OpenCode Console/Zen cloud gateway. For a local OpenCode server choose OpenCode local.",
    },
    ProviderPreset {
        id: "opencode-local",
        name: "OpenCode local",
        protocol: Protocol::OpenCodeServer,
        base_url: "http://localhost:4096",
        model: "",
        key_env: "OPENCODE_PASSWORD",
        needs_key: false,
        note: "Local `opencode serve` (default port 4096). Optional model as provider/model; set OPENCODE_PASSWORD as the API key if the server uses Basic auth.",
    },
    ProviderPreset {
        id: "openrouter",
        name: "OpenRouter",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1",
        model: "openrouter/auto",
        key_env: "OPENROUTER_API_KEY",
        needs_key: true,
        note: "OpenRouter model router.",
    },
    ProviderPreset {
        id: "groq",
        name: "Groq",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.groq.com/openai/v1",
        model: "llama-3.3-70b-versatile",
        key_env: "GROQ_API_KEY",
        needs_key: true,
        note: "Groq fast inference.",
    },
    ProviderPreset {
        id: "deepseek",
        name: "DeepSeek",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
        key_env: "DEEPSEEK_API_KEY",
        needs_key: true,
        note: "DeepSeek API.",
    },
    ProviderPreset {
        id: "mistral",
        name: "Mistral",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.mistral.ai/v1",
        model: "mistral-large-latest",
        key_env: "MISTRAL_API_KEY",
        needs_key: true,
        note: "Mistral AI API.",
    },
    ProviderPreset {
        id: "gemini",
        name: "Google Gemini",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-2.5-flash",
        key_env: "GEMINI_API_KEY",
        needs_key: true,
        note: "Google Gemini through its OpenAI-compatible endpoint.",
    },
    ProviderPreset {
        id: "xai",
        name: "xAI",
        protocol: Protocol::OpenAiCompatible,
        base_url: "https://api.x.ai/v1",
        model: "grok-4",
        key_env: "XAI_API_KEY",
        needs_key: true,
        note: "xAI Grok API.",
    },
    ProviderPreset {
        id: "ollama",
        name: "Ollama (local)",
        protocol: Protocol::OpenAiCompatible,
        base_url: "http://localhost:11434/v1",
        model: "llama3.2",
        key_env: "OLLAMA_API_KEY",
        needs_key: false,
        note: "Local Ollama server; no API key required.",
    },
    ProviderPreset {
        id: "lmstudio",
        name: "LM Studio (local)",
        protocol: Protocol::OpenAiCompatible,
        base_url: "http://127.0.0.1:1234/v1",
        model: "local-model",
        key_env: "LMSTUDIO_API_KEY",
        needs_key: false,
        note: "Local LM Studio server; no API key required.",
    },
    ProviderPreset {
        id: "custom",
        name: "Custom / local",
        protocol: Protocol::OpenAiCompatible,
        base_url: "",
        model: "",
        key_env: "MAPPAMENTE_API_KEY",
        needs_key: false,
        note: "Any OpenAI-compatible endpoint, including a local OpenCode server.",
    },
];

/// Returns the preset with `id`, if any.
#[must_use]
pub fn preset_by_id(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

/// Settings used for one request.
#[derive(Clone, Debug)]
pub struct ProviderSettings {
    /// Stable provider id.
    pub id: String,
    /// Human-readable provider name, used in errors.
    pub name: String,
    /// Request/response protocol.
    pub protocol: Protocol,
    /// Base URL supplied by the user.
    pub base_url: String,
    /// Model id supplied by the user.
    pub model: String,
    /// API key, if available.
    pub api_key: Option<String>,
    /// Environment variable to mention when the key is missing.
    pub key_env: String,
    /// Whether the key is required.
    pub needs_key: bool,
}

impl ProviderSettings {
    /// Creates settings from a preset and no API key.
    #[must_use]
    pub fn from_preset(preset: &ProviderPreset) -> Self {
        Self {
            id: preset.id.to_owned(),
            name: preset.name.to_owned(),
            protocol: preset.protocol,
            base_url: preset.base_url.to_owned(),
            model: preset.model.to_owned(),
            api_key: None,
            key_env: preset.key_env.to_owned(),
            needs_key: preset.needs_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn presets_are_unique_and_complete() {
        assert!(PRESETS.len() >= 13);
        let mut ids = HashSet::new();
        for preset in PRESETS {
            assert!(ids.insert(preset.id), "duplicate id {}", preset.id);
            assert!(!preset.name.is_empty());
            assert!(!preset.key_env.is_empty());
            if preset.id != "custom" {
                assert!(!preset.base_url.is_empty(), "{} has no base URL", preset.id);
            }
            if !matches!(preset.id, "custom" | "opencode-local") {
                assert!(!preset.model.is_empty(), "{} has no model", preset.id);
            }
        }
        assert!(preset_by_id("openai").is_some());
        assert!(preset_by_id("missing").is_none());
    }

    #[test]
    fn local_presets_do_not_require_keys() {
        for id in ["ollama", "lmstudio", "custom", "opencode-local"] {
            assert!(!preset_by_id(id).unwrap().needs_key, "{id}");
        }
        for id in ["openai", "anthropic", "opencode"] {
            assert!(preset_by_id(id).unwrap().needs_key, "{id}");
        }
    }
}
