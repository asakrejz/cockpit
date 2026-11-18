//! The sections of the finished configuration and the error.

use super::*;

/// The tool and its endpoint as last configured (replaceable during operation).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentChoice {
    pub provider: Provider,
    pub endpoint: Option<EndpointConfig>,
}

/// The model endpoint of the agent. The key is in the .env file, never here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndpointConfig {
    /// Without a slash at the end, e.g. `http://localhost:4000`; `None` = directly at the provider.
    pub url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubConfig {
    pub api_url: String,
    /// `owner/name`; without repos GitHub does not sync.
    pub repos: Vec<String>,
}

/// The GitHub App for the sign-in. The client secret is in the .env file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
}

/// The Atlassian app (OAuth 2.0 3LO). The client secret is in the .env file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlassianConfig {
    pub client_id: String,
    /// Which site, if several are approved (name, address or id).
    pub site: Option<String>,
}

/// With OAuth `site` and `email` are empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraConfig {
    pub site: String,
    pub email: String,
    pub projects: Vec<String>,
    pub jql: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, std::io::Error),
    Toml(String),
    /// A value that is not allowed; the message is shown to people (setup) or logged in English.
    Wert(cockpit_i18n::Msg),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read(p, e) => {
                write!(f, "Configuration file {} not readable: {e}", p.display())
            }
            ConfigError::Toml(m) => write!(f, "Configuration file invalid: {m}"),
            ConfigError::Wert(m) => {
                write!(f, "Invalid value: {}", m.render(cockpit_i18n::Lang::En))
            }
        }
    }
}

impl std::error::Error for ConfigError {}
