//! Storage of the keys. They are in the `.env` file in the folder `config/` of the project (`EnvFileStore`, never
//! commit it), never in the database, never in the configuration file. `Secret` only shows as
//! `Secret(***)` in `Debug`.

mod env_check;
mod env_file;

pub use env_check::{EnvFileStatus, status as env_file_status};
pub use env_file::{DEFAULT_FILE_NAME as ENV_FILE_NAME, ENV_FILE_VAR, EnvFileStore};

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use cockpit_integrations::Secret;

/// Names of the keys that the backend knows.
pub const GITHUB: &str = "github";
pub const JIRA: &str = "jira";
/// Client secrets of the OAuth apps (`token set github-oauth|atlassian-oauth`).
pub const GITHUB_OAUTH: &str = "github-oauth";
pub const ATLASSIAN_OAUTH: &str = "atlassian-oauth";
/// Key for the endpoint of the agent (`token set agent-key`); only goes to the agent container at start.
pub const AGENT_KEY: &str = "agent-key";
/// Shared secrets for the signature of incoming webhooks (`GITHUB_WEBHOOK_SECRET`,
/// `JIRA_WEBHOOK_SECRET`); the assistant can generate them.
pub const GITHUB_WEBHOOK: &str = "github-webhook";
pub const JIRA_WEBHOOK: &str = "jira-webhook";
/// Access and refresh token after the sign-in (one entry per service, set by the backend).
pub const GITHUB_OAUTH_TOKENS: &str = "github-oauth-token";
pub const ATLASSIAN_OAUTH_TOKENS: &str = "atlassian-oauth-token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretError(pub String);

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key storage: {}", self.0)
    }
}

impl std::error::Error for SecretError {}

pub trait SecretStore: Send + Sync {
    /// `Ok(None)` if there is no key with this name.
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretError>;
    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError>;
    /// Deleting a key that does not exist is no error.
    fn delete(&self, name: &str) -> Result<(), SecretError>;

    fn has(&self, name: &str) -> Result<bool, SecretError> {
        Ok(self.get(name)?.is_some())
    }

    /// The file that holds the keys (only `EnvFileStore`); for status and warnings.
    fn env_file(&self) -> Option<&std::path::Path> {
        None
    }
}

/// In memory only – for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, Secret>>);

#[cfg(test)]
impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Only the names, never the values.
        let names: Vec<String> = self
            .0
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        f.debug_struct("MemoryStore")
            .field("namen", &names)
            .finish()
    }
}

#[cfg(test)]
impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretError> {
        Ok(self.0.lock().expect("lock poisoned").get(name).cloned())
    }

    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError> {
        self.0
            .lock()
            .expect("lock poisoned")
            .insert(name.into(), value.clone());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.0.lock().expect("lock poisoned").remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
