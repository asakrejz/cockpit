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
    /// `owner/name`: the "watched" repos, of which all open pull requests are loaded. Without
    /// repos the sync still finds the pull requests of the signed-in user by search.
    pub repos: Vec<String>,
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            api_url: super::DEFAULT_GITHUB_API.into(),
            repos: Vec::new(),
        }
    }
}

/// `[github_projects]`: GitHub Projects as the source of tickets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitHubProjectsConfig {
    pub tickets_from: cockpit_core::TicketsFrom,
    pub projects: Vec<cockpit_core::ProjectRef>,
    /// The field that counts as the ticket status; `None` = `Status`.
    pub status_field: Option<String>,
    /// The field that holds the team; `None` = `Team`.
    pub team_field: Option<String>,
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
    /// Fields (name or ID) the detail view leaves out.
    pub hidden_fields: Vec<String>,
    /// Fields (name or ID) the detail view shows first, in this order.
    pub field_order: Vec<String>,
    /// The ID of the Jira „Team“ field (`customfield_10400`), if the detection fails on the site.
    pub team_field: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, std::io::Error),
    Toml(String),
    /// A key the file must not have: a typo or a name this version does not know. `section` is
    /// empty at the top of the file, `file` is the file it stands in.
    UnknownKey {
        section: String,
        key: String,
        file: PathBuf,
    },
    /// A value that is not allowed; the message is shown to people (setup) or logged in English.
    Wert(cockpit_i18n::Msg),
}

impl ConfigError {
    /// Names the file the error was found in (the text alone does not know its path).
    pub fn in_file(self, path: &Path) -> Self {
        match self {
            ConfigError::UnknownKey { section, key, .. } => ConfigError::UnknownKey {
                section,
                key,
                file: path.to_path_buf(),
            },
            other => other,
        }
    }

    /// The error as a message for people, in the language of whoever reads it.
    pub fn msg(&self) -> Msg {
        match self {
            ConfigError::Wert(m) => m.clone(),
            ConfigError::UnknownKey { section, key, file } if section.is_empty() => msg!(
                ServerConfigKeyUnknownTop,
                key = key,
                file = file.display().to_string()
            ),
            ConfigError::UnknownKey { section, key, file } => msg!(
                ServerConfigKeyUnknown,
                key = key,
                section = section,
                file = file.display().to_string()
            ),
            other => Msg::raw(other.to_string()),
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read(p, e) => {
                write!(f, "Configuration file {} not readable: {e}", p.display())
            }
            ConfigError::Toml(m) => write!(f, "Configuration file invalid: {m}"),
            ConfigError::UnknownKey { .. } => {
                write!(f, "{}", self.msg().render(cockpit_i18n::Lang::En))
            }
            ConfigError::Wert(m) => {
                write!(f, "Invalid value: {}", m.render(cockpit_i18n::Lang::En))
            }
        }
    }
}

impl std::error::Error for ConfigError {}
