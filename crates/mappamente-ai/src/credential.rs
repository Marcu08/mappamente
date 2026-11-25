//! API-key storage: OS keyring first, environment variable fallback.
//!
//! Keys are never written to map files, settings files or logs. On platforms
//! where the credential store is unavailable (for example a headless Linux
//! session without Secret Service), callers fall back to the documented
//! environment variable for the provider.

use keyring::v1::Entry;

use crate::error::{AiError, Result};
use crate::provider::ProviderPreset;

const SERVICE: &str = "mappamente";

/// Where the current API key came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// A key is stored in the OS credential store.
    Keyring,
    /// A key was read from the provider's environment variable.
    Environment,
    /// No key was found.
    Missing,
}

/// Loads the key for `preset`: keyring first, then environment.
#[must_use]
pub fn load_api_key(preset: &ProviderPreset) -> Option<String> {
    if let Ok(entry) = Entry::new(SERVICE, preset.id)
        && let Ok(password) = entry.get_password()
        && !password.trim().is_empty()
    {
        return Some(password.trim().to_owned());
    }
    environment_key(preset)
}

/// Reports where the key for `preset` would come from without exposing it.
#[must_use]
pub fn key_source(preset: &ProviderPreset) -> KeySource {
    if let Ok(entry) = Entry::new(SERVICE, preset.id)
        && let Ok(password) = entry.get_password()
        && !password.trim().is_empty()
    {
        return KeySource::Keyring;
    }
    if environment_key(preset).is_some() {
        KeySource::Environment
    } else {
        KeySource::Missing
    }
}

/// Stores `key` in the OS credential store.
pub fn save_api_key(preset: &ProviderPreset, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AiError::InvalidSettings("the API key is empty".to_owned()));
    }
    let entry =
        Entry::new(SERVICE, preset.id).map_err(|error| AiError::Credential(error.to_string()))?;
    entry
        .set_password(key)
        .map_err(|error| AiError::Credential(error.to_string()))
}

/// Deletes the stored key. A missing entry is not an error.
pub fn clear_api_key(preset: &ProviderPreset) -> Result<()> {
    let entry =
        Entry::new(SERVICE, preset.id).map_err(|error| AiError::Credential(error.to_string()))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
        Err(error) => Err(AiError::Credential(error.to_string())),
    }
}

fn environment_key(preset: &ProviderPreset) -> Option<String> {
    let key = std::env::var(preset.key_env)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());
    if key.is_some() {
        return key;
    }
    if preset.id == "opencode-local" {
        return std::env::var("OPENCODE_SERVER_PASSWORD")
            .ok()
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty());
    }
    None
}
