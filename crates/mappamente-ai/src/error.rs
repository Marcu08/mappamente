//! Errors returned by the AI integration.

use thiserror::Error;

/// Result alias for provider calls and plan parsing.
pub type Result<T> = std::result::Result<T, AiError>;

/// Everything that can go wrong before or during an AI request.
#[derive(Debug, Error)]
pub enum AiError {
    /// A cloud provider needs an API key but none was found.
    #[error(
        "no API key for {provider}; enter one in the AI panel or set the {env} environment variable"
    )]
    MissingApiKey {
        /// Provider name.
        provider: String,
        /// Environment variable checked as a fallback.
        env: String,
    },

    /// The provider, model or URL is invalid.
    #[error("invalid AI settings: {0}")]
    InvalidSettings(String),

    /// The map outline is too large to send to a model.
    #[error("the map is too large for the AI context ({chars} characters; maximum {limit})")]
    ContextTooLarge {
        /// Actual outline length.
        chars: usize,
        /// Maximum accepted length.
        limit: usize,
    },

    /// The OS credential store is unavailable or rejected the operation.
    #[error("cannot access the OS credential store: {0}")]
    Credential(String),

    /// The HTTP request failed before a response was available.
    #[error("network error: {0}")]
    Network(String),

    /// The provider returned a non-success HTTP status.
    #[error("{provider} returned HTTP {status}: {message}")]
    Http {
        /// Provider name.
        provider: String,
        /// HTTP status code.
        status: u16,
        /// Provider error message.
        message: String,
    },

    /// The model response was not JSON in the expected shape.
    #[error("the AI response was not valid JSON: {0}")]
    InvalidResponse(String),

    /// An action was structurally valid but could not be applied.
    #[error("invalid AI action: {0}")]
    InvalidAction(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_message_names_the_environment_variable() {
        let error = AiError::MissingApiKey {
            provider: "OpenAI".to_owned(),
            env: "OPENAI_API_KEY".to_owned(),
        };
        assert!(error.to_string().contains("OPENAI_API_KEY"));
        assert!(error.to_string().contains("OpenAI"));
    }
}
