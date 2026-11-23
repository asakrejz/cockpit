//! Wire shapes of the setup and configuration endpoints.

use super::*;

/// Body of `PUT /api/secrets/{name}`. The value is a secret: it only stands in the input field,
/// goes once to the local server and never comes back. `Debug` does not show it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub value: String,
}

impl std::fmt::Debug for KeyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SchluesselEingabe { wert: *** }")
    }
}

/// Whether a key is in the .env file – never its value.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KeyResponse {
    pub name: String,
    #[serde(default)]
    pub set: bool,
}

/// The client ID of an OAuth app (not secret). Empty removes the section.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientInput {
    #[serde(default)]
    pub client_id: String,
}

/// The GitHub repos for the sync (`owner/name`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHubInput {
    #[serde(default)]
    pub repos: Vec<String>,
}

/// `[github_projects]`: where tickets come from and which projects. `None` leaves a value as
/// it is; an empty list removes the projects, an empty field name the choice.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHubProjectsInput {
    #[serde(default)]
    pub tickets_from: Option<crate::TicketsFrom>,
    /// `owner/number`, e.g. `acme/3`.
    #[serde(default)]
    pub projects: Option<Vec<String>>,
    #[serde(default)]
    pub status_field: Option<String>,
    #[serde(default)]
    pub team_field: Option<String>,
}

/// The non-secret `[github_projects]` values for the assistant.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHubProjectsSetup {
    #[serde(default)]
    pub tickets_from: crate::TicketsFrom,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub status_field: Option<String>,
    #[serde(default)]
    pub team_field: Option<String>,
}

/// What the credential may do with GitHub Projects, as far as it is known.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectRights {
    #[serde(default)]
    pub kind: crate::TokenKind,
    /// The scopes of a classic token; `None` = GitHub does not say (fine-grained token, App).
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// Rights known to be missing (classic tokens only), each with the plain-language fix.
    #[serde(default)]
    pub missing: Vec<MissingRight>,
}

/// A right the credential lacks, and how to get it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MissingRight {
    pub right: crate::Right,
    pub msg: cockpit_i18n::Msg,
}

/// Response to `GET /api/github/projects`: the projects the credential can see.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectsResponse {
    /// GitHub is connected at all.
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub projects: Vec<crate::ProjectSummary>,
    #[serde(default)]
    pub rights: ProjectRights,
    /// Why the list could not be read (a missing right, a quota, no network).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<cockpit_i18n::Msg>,
    /// The logins the list can be created for: the user and their organizations.
    #[serde(default)]
    pub owners: Vec<String>,
}

/// Body of `POST /api/github/projects/create`: a new project for `owner` (the user or an
/// organization), optionally linked to `repo`. Only sent when the user clicks "Projekt anlegen".
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CreateProjectRequest {
    pub owner: String,
    pub title: String,
    #[serde(default)]
    pub repo: Option<String>,
}

/// Response to `POST /api/github/projects/create`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CreateProjectResponse {
    pub project: crate::ProjectSummary,
}

/// Provider, address and model of the agent. `None` leaves a value unchanged; an empty
/// address means „directly at the provider“, an empty model removes the name.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointInput {
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Body of `PUT /api/config`: only the sections that change. Keys do not belong
/// here but to `PUT /api/secrets/{name}`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira: Option<JiraInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlassian: Option<ClientInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_oauth: Option<ClientInput>,
    /// `[atlassian] site`: which site to use if several are approved (name, address or id).
    /// Empty removes the value; needs `[atlassian] client_id` (set earlier or in the same change).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlassian_site: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GitHubInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_repos: Option<AgentReposInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<EndpointInput>,
    /// `[github] auth`: the chosen sign-in method for GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_auth: Option<crate::AuthChoice>,
    /// `[jira] auth`: the chosen sign-in method for Jira.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira_auth: Option<crate::AuthChoice>,
    /// `[server] webhooks` and `[server] public_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhooks: Option<WebhooksInput>,
    /// `[github_projects]`: GitHub Projects as the source of tickets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_projects: Option<GitHubProjectsInput>,
}

/// Response to `PUT /api/config`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigResponse {
    /// `config.toml` is written.
    #[serde(default)]
    pub stored: bool,
    /// The change already applies now; otherwise a restart is needed.
    #[serde(default)]
    pub takes_effect_immediately: bool,
    /// What the user should know.
    #[serde(default)]
    pub hint: Option<String>,
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

/// Response to `GET /api/setup`: the current, non-secret values for the assistant.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    #[serde(default)]
    pub jira_site: String,
    #[serde(default)]
    pub jira_email: String,
    #[serde(default)]
    pub jira_projects: Vec<String>,
    #[serde(default)]
    pub atlassian_client_id: Option<String>,
    /// `[atlassian] site`, if set.
    #[serde(default)]
    pub atlassian_site: Option<String>,
    #[serde(default)]
    pub github_client_id: Option<String>,
    #[serde(default)]
    pub github_repos: Vec<String>,
    /// `[[agent.repos]]` and `[agent] repo`: where the agent works and which Jira projects go where.
    #[serde(default)]
    pub agent_repos: AgentRepos,
    #[serde(default)]
    pub provider: Provider,
    /// Address of the endpoint; `None` = directly at the provider.
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default)]
    pub endpoint_model: Option<String>,
    /// The address entered as callback in the GitHub App.
    #[serde(default)]
    pub callback_github: String,
    /// The address entered as callback in the Atlassian app.
    #[serde(default)]
    pub callback_atlassian: String,
    /// Which keys are in the .env file.
    #[serde(default)]
    pub keys: Vec<KeyResponse>,
    /// Optional webhooks (live update).
    #[serde(default)]
    pub webhooks: WebhooksInfo,
    /// GitHub Projects as a source of tickets.
    #[serde(default)]
    pub github_projects: GitHubProjectsSetup,
}

impl Setup {
    /// Is the key `name` in the .env file?
    pub fn has_key(&self, name: &str) -> bool {
        self.keys.iter().any(|s| s.name == name && s.set)
    }
}
