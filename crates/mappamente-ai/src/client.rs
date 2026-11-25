//! Blocking HTTP clients for OpenAI-compatible and Anthropic providers.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::{Client, Response};
use serde::Serialize;
use serde_json::Value;

use crate::error::{AiError, Result};
use crate::provider::{Protocol, ProviderSettings};

/// Total request timeout, including the model response.
pub const REQUEST_TIMEOUT_SECS: u64 = 120;

/// Maximum output tokens requested from Anthropic.
pub const MAX_OUTPUT_TOKENS: u32 = 8_192;

/// Chat role sent to providers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The user's request.
    User,
    /// A previous assistant reply.
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// One conversation message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    /// Who wrote the message.
    pub role: Role,
    /// Plain text body.
    pub content: String,
}

impl ChatMessage {
    /// Creates a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Creates an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// One provider request.
#[derive(Clone, Debug)]
pub struct ChatRequest {
    /// Provider, model and endpoint settings.
    pub settings: ProviderSettings,
    /// System prompt, including the current map.
    pub system: String,
    /// Conversation messages, oldest first.
    pub messages: Vec<ChatMessage>,
}

/// Token usage reported by the provider, when available.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Input/prompt tokens.
    pub input: Option<u64>,
    /// Output/completion tokens.
    pub output: Option<u64>,
}

/// A successful provider response.
#[derive(Clone, Debug)]
pub struct ChatResponse {
    /// Text returned by the model.
    pub text: String,
    /// Token usage, when the provider reports it.
    pub usage: TokenUsage,
}

/// Reusable blocking HTTP client.
pub struct ProviderClient {
    client: Client,
}

impl ProviderClient {
    /// Builds a client with the project user agent, a timeout and no redirects
    /// (so an API key cannot be forwarded to another endpoint).
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent(concat!("mappamente/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(network_error)?;
        Ok(Self { client })
    }

    /// Sends one request and parses the provider response.
    pub fn send(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let endpoint = endpoint(&request.settings)?;
        let response = match request.settings.protocol {
            Protocol::OpenAiCompatible => self.send_openai(endpoint, request)?,
            Protocol::Anthropic => self.send_anthropic(endpoint, request)?,
            Protocol::OpenCodeServer => self.send_opencode(endpoint, request)?,
        };
        let status = response.status();
        let body = response.text().map_err(network_error)?;
        if !status.is_success() {
            return Err(AiError::Http {
                provider: request.settings.name.clone(),
                status: status.as_u16(),
                message: provider_message(&body),
            });
        }
        match request.settings.protocol {
            Protocol::OpenAiCompatible => parse_openai(&body),
            Protocol::Anthropic => parse_anthropic(&body),
            Protocol::OpenCodeServer => parse_opencode(&body),
        }
    }

    fn send_openai(&self, endpoint: Url, request: &ChatRequest) -> Result<Response> {
        let body = openai_body(request);
        let mut builder = self.client.post(endpoint).json(&body);
        if let Some(key) = api_key(&request.settings) {
            builder = builder.bearer_auth(key);
        }
        builder.send().map_err(network_error)
    }

    fn send_anthropic(&self, endpoint: Url, request: &ChatRequest) -> Result<Response> {
        let body = anthropic_body(request);
        let mut builder = self
            .client
            .post(endpoint)
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        if let Some(key) = api_key(&request.settings) {
            builder = builder.header("x-api-key", key);
        }
        builder.send().map_err(network_error)
    }

    fn send_opencode(&self, endpoint: Url, request: &ChatRequest) -> Result<Response> {
        let prompt = opencode_prompt(request);
        let model = opencode_model(&request.settings.model);
        let body = OpenCodeBody {
            prompt: &prompt,
            model,
        };
        let mut builder = self.client.post(endpoint).json(&body);
        if let Some(key) = api_key(&request.settings) {
            builder = builder.basic_auth("opencode", Some(key));
        }
        builder.send().map_err(network_error)
    }
}

/// Validates the settings and builds the provider endpoint URL.
pub fn endpoint(settings: &ProviderSettings) -> Result<Url> {
    if settings.model.trim().is_empty() && settings.protocol != Protocol::OpenCodeServer {
        return Err(AiError::InvalidSettings("the model is empty".to_owned()));
    }
    if settings.protocol == Protocol::OpenCodeServer
        && !settings.model.trim().is_empty()
        && !settings.model.contains('/')
    {
        return Err(AiError::InvalidSettings(
            "OpenCode local models must use the provider/model form, e.g. opencode/gpt-5"
                .to_owned(),
        ));
    }
    if settings.needs_key && api_key(settings).is_none() {
        return Err(AiError::MissingApiKey {
            provider: settings.name.clone(),
            env: settings.key_env.clone(),
        });
    }
    if settings.base_url.trim().is_empty() {
        return Err(AiError::InvalidSettings(
            "the base URL is empty; enter the provider endpoint".to_owned(),
        ));
    }

    let base = Url::parse(settings.base_url.trim())
        .map_err(|error| AiError::InvalidSettings(format!("invalid base URL: {error}")))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(AiError::InvalidSettings(
            "the base URL must use http or https".to_owned(),
        ));
    }
    if base.scheme() == "http" && !is_loopback(&base) {
        return Err(AiError::InvalidSettings(
            "plain http is only allowed for localhost; use https for remote providers".to_owned(),
        ));
    }
    if !base.username().is_empty() || base.password().is_some() {
        return Err(AiError::InvalidSettings(
            "do not put credentials in the base URL; use the API key field".to_owned(),
        ));
    }
    if base.query().is_some() || base.fragment().is_some() {
        return Err(AiError::InvalidSettings(
            "the base URL cannot contain a query or fragment".to_owned(),
        ));
    }

    let mut base = base.as_str().trim_end_matches('/').to_owned();
    let path = match settings.protocol {
        Protocol::OpenAiCompatible => {
            if base.ends_with("/chat/completions") {
                return parse_endpoint(&base);
            }
            "/chat/completions"
        }
        Protocol::Anthropic => {
            if base.ends_with("/messages") {
                return parse_endpoint(&base);
            }
            if base.ends_with("/v1") {
                "/messages"
            } else {
                "/v1/messages"
            }
        }
        Protocol::OpenCodeServer => {
            if base.ends_with("/api/experimental/generate/text") {
                return parse_endpoint(&base);
            }
            if base.ends_with("/api") {
                "/experimental/generate/text"
            } else {
                "/api/experimental/generate/text"
            }
        }
    };
    base.push_str(path);
    parse_endpoint(&base)
}

fn parse_endpoint(value: &str) -> Result<Url> {
    Url::parse(value)
        .map_err(|error| AiError::InvalidSettings(format!("invalid endpoint URL: {error}")))
}

fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn api_key(settings: &ProviderSettings) -> Option<&str> {
    settings
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
}

#[derive(Serialize)]
struct OpenAiBody<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'static str,
    content: &'a str,
}

fn openai_body<'a>(request: &'a ChatRequest) -> OpenAiBody<'a> {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    messages.push(OpenAiMessage {
        role: "system",
        content: &request.system,
    });
    messages.extend(request.messages.iter().map(|message| OpenAiMessage {
        role: message.role.as_str(),
        content: &message.content,
    }));
    OpenAiBody {
        model: request.settings.model.trim(),
        messages,
    }
}

#[derive(Serialize)]
struct AnthropicBody<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'static str,
    content: &'a str,
}

fn anthropic_body<'a>(request: &'a ChatRequest) -> AnthropicBody<'a> {
    AnthropicBody {
        model: request.settings.model.trim(),
        max_tokens: MAX_OUTPUT_TOKENS,
        system: &request.system,
        messages: request
            .messages
            .iter()
            .map(|message| AnthropicMessage {
                role: message.role.as_str(),
                content: &message.content,
            })
            .collect(),
    }
}

#[derive(Serialize)]
struct OpenCodeBody<'a> {
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<OpenCodeModel<'a>>,
}

#[derive(Serialize)]
struct OpenCodeModel<'a> {
    #[serde(rename = "providerID")]
    provider_id: &'a str,
    id: &'a str,
}

fn opencode_model(model: &str) -> Option<OpenCodeModel<'_>> {
    let (provider_id, id) = model.trim().split_once('/')?;
    if provider_id.is_empty() || id.is_empty() {
        return None;
    }
    Some(OpenCodeModel { provider_id, id })
}

fn opencode_prompt(request: &ChatRequest) -> String {
    let mut prompt = String::with_capacity(request.system.len() + 256);
    prompt.push_str("System instructions:\n");
    prompt.push_str(&request.system);
    prompt.push_str("\n\nConversation:\n");
    for message in &request.messages {
        match message.role {
            Role::User => prompt.push_str("User: "),
            Role::Assistant => prompt.push_str("Assistant: "),
        }
        prompt.push_str(&message.content);
        prompt.push('\n');
    }
    prompt
}

fn parse_opencode(body: &str) -> Result<ChatResponse> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| AiError::InvalidResponse(format!("OpenCode JSON error: {error}")))?;
    if let Some(message) = provider_error_value(&value) {
        return Err(AiError::InvalidResponse(message));
    }
    let text = value
        .pointer("/data/text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| AiError::InvalidResponse("OpenCode response has no data.text".to_owned()))?;
    Ok(ChatResponse {
        text,
        usage: TokenUsage::default(),
    })
}

fn parse_openai(body: &str) -> Result<ChatResponse> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| AiError::InvalidResponse(format!("OpenAI JSON error: {error}")))?;
    if let Some(message) = provider_error_value(&value) {
        return Err(AiError::InvalidResponse(message));
    }
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(content_text)
        .or_else(|| {
            value
                .pointer("/choices/0/text")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value
                .pointer("/output_text")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| {
            AiError::InvalidResponse("OpenAI response has no message content".to_owned())
        })?;
    if text.trim().is_empty() {
        return Err(AiError::InvalidResponse(
            "OpenAI returned an empty message".to_owned(),
        ));
    }
    Ok(ChatResponse {
        text,
        usage: TokenUsage {
            input: value
                .pointer("/usage/prompt_tokens")
                .and_then(Value::as_u64),
            output: value
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_u64),
        },
    })
}

fn parse_anthropic(body: &str) -> Result<ChatResponse> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| AiError::InvalidResponse(format!("Anthropic JSON error: {error}")))?;
    if let Some(message) = provider_error_value(&value) {
        return Err(AiError::InvalidResponse(message));
    }
    let text = value
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            AiError::InvalidResponse("Anthropic response has no text content".to_owned())
        })?;
    Ok(ChatResponse {
        text,
        usage: TokenUsage {
            input: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
            output: value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
        },
    })
}

fn content_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.as_str())
                })
                .collect::<Vec<_>>()
                .join("");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn provider_error_value(value: &Value) -> Option<String> {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn provider_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(message) = provider_error_value(&value)
    {
        return truncate(&message);
    }
    truncate(body)
}

fn truncate(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() > 800 {
        text.chars().take(800).collect::<String>() + "…"
    } else {
        text.to_owned()
    }
}

fn network_error(error: reqwest::Error) -> AiError {
    AiError::Network(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{PRESETS, ProviderPreset, preset_by_id};

    fn settings(preset: &ProviderPreset) -> ProviderSettings {
        let mut settings = ProviderSettings::from_preset(preset);
        settings.api_key = Some("test-key".to_owned());
        settings
    }

    #[test]
    fn builds_openai_and_anthropic_endpoints() {
        let openai = settings(preset_by_id("openai").unwrap());
        assert_eq!(
            endpoint(&openai).unwrap().as_str(),
            "https://api.openai.com/v1/chat/completions"
        );

        let anthropic = settings(preset_by_id("anthropic").unwrap());
        assert_eq!(
            endpoint(&anthropic).unwrap().as_str(),
            "https://api.anthropic.com/v1/messages"
        );

        let opencode = settings(preset_by_id("opencode").unwrap());
        assert_eq!(
            endpoint(&opencode).unwrap().as_str(),
            "https://opencode.ai/inference/openai/v1/chat/completions"
        );

        let mut local = settings(preset_by_id("opencode-local").unwrap());
        local.model.clear();
        local.api_key = None;
        assert_eq!(
            endpoint(&local).unwrap().as_str(),
            "http://localhost:4096/api/experimental/generate/text"
        );
    }

    #[test]
    fn accepts_local_http_and_rejects_remote_http() {
        let mut local = settings(preset_by_id("ollama").unwrap());
        local.api_key = None;
        local.needs_key = false;
        assert!(endpoint(&local).is_ok());

        let mut remote = settings(preset_by_id("openai").unwrap());
        remote.base_url = "http://api.openai.com/v1".to_owned();
        let error = endpoint(&remote).unwrap_err();
        assert!(error.to_string().contains("localhost"));
    }

    #[test]
    fn rejects_missing_keys_and_empty_models() {
        let mut openai = ProviderSettings::from_preset(preset_by_id("openai").unwrap());
        openai.api_key = None;
        assert!(matches!(
            endpoint(&openai),
            Err(AiError::MissingApiKey { .. })
        ));

        let mut missing_model = settings(preset_by_id("openai").unwrap());
        missing_model.model.clear();
        assert!(matches!(
            endpoint(&missing_model),
            Err(AiError::InvalidSettings(_))
        ));
    }

    #[test]
    fn request_bodies_include_system_and_history() {
        let request = ChatRequest {
            settings: settings(preset_by_id("openai").unwrap()),
            system: "system".to_owned(),
            messages: vec![ChatMessage::user("hello"), ChatMessage::assistant("hi")],
        };
        let openai = serde_json::to_value(openai_body(&request)).unwrap();
        assert_eq!(openai["model"], "gpt-5.2");
        assert_eq!(openai["messages"][0]["role"], "system");
        assert_eq!(openai["messages"][1]["content"], "hello");
        assert_eq!(openai["messages"][2]["role"], "assistant");

        let anthropic = serde_json::to_value(anthropic_body(&request)).unwrap();
        assert_eq!(anthropic["system"], "system");
        assert_eq!(anthropic["max_tokens"], MAX_OUTPUT_TOKENS);
        assert_eq!(anthropic["messages"][0]["role"], "user");
        assert_eq!(anthropic["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn parses_openai_string_and_array_content() {
        let body = r#"{"choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;
        let response = parse_openai(body).unwrap();
        assert_eq!(response.text, "hello");
        assert_eq!(response.usage.input, Some(3));
        assert_eq!(response.usage.output, Some(2));

        let body = r#"{"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]}"#;
        assert_eq!(parse_openai(body).unwrap().text, "ab");
    }

    #[test]
    fn parses_anthropic_text_blocks() {
        let body = r#"{"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"x"}],"usage":{"input_tokens":4,"output_tokens":1}}"#;
        let response = parse_anthropic(body).unwrap();
        assert_eq!(response.text, "hello");
        assert_eq!(response.usage.input, Some(4));
        assert_eq!(response.usage.output, Some(1));
    }

    #[test]
    fn opencode_local_prompt_model_and_response() {
        let mut provider = ProviderSettings::from_preset(preset_by_id("opencode-local").unwrap());
        provider.model = "opencode/gpt-5".to_owned();
        let request = ChatRequest {
            settings: provider,
            system: "system".to_owned(),
            messages: vec![ChatMessage::user("hello")],
        };

        let prompt = opencode_prompt(&request);
        assert!(prompt.contains("System instructions:\nsystem"));
        assert!(prompt.contains("User: hello"));

        let model = opencode_model(&request.settings.model).unwrap();
        assert_eq!(model.provider_id, "opencode");
        assert_eq!(model.id, "gpt-5");
        let body = serde_json::to_value(OpenCodeBody {
            prompt: &prompt,
            model: Some(model),
        })
        .unwrap();
        assert_eq!(body["model"]["providerID"], "opencode");
        assert_eq!(body["model"]["id"], "gpt-5");
        assert!(body["prompt"].as_str().unwrap().contains("User: hello"));

        assert_eq!(
            parse_opencode(r#"{"data":{"text":"hello"}}"#).unwrap().text,
            "hello"
        );
    }

    #[test]
    fn surfaces_provider_error_messages() {
        let error = parse_openai(r#"{"error":{"message":"bad key"}}"#).unwrap_err();
        assert!(error.to_string().contains("bad key"));
        assert_eq!(provider_message(r#"{"error":{"message":"nope"}}"#), "nope");
        assert!(provider_message("plain failure").contains("plain failure"));
    }

    #[test]
    fn all_cloud_presets_validate_with_a_key() {
        for preset in PRESETS {
            if preset.id == "custom" {
                continue;
            }
            assert!(
                endpoint(&settings(preset)).is_ok(),
                "{} endpoint failed",
                preset.id
            );
        }
    }
}
