//! The JSON shapes between app and backend (server mode).
//!
//! * `GET /api/workspace`: the state in the shape of [`persist`](crate::persist), plus `revision`
//! * `POST /api/actions`: body [`ActionRequest`], response [`ActionResponse`] or [`ErrorBody`]
//! * `GET /api/events`: server-sent events, each event carries a [`ServerEvent`]
//! * `GET /api/oauth/{github|atlassian}/start` and `…/callback`: sign-in via OAuth (navigation of the
//!   browser, redirects to `/`); `POST …/revoke`: [`RevokeResponse`]
//! * `GET /api/jira/projects`: the Jira projects for the selection, [`JiraProjectsResponse`]
//! * `GET /api/agent/models`: the model IDs of the endpoint for the drop-down, [`ModelList`]
//! * `GET /api/agent/check`: connection test to the endpoint, [`EndpointCheck`]
//! * `POST /api/jira/check`: connection test for Jira via API token (read only), [`JiraCheck`]
//! * `DELETE /api/secrets/{name}`: deletes a token (`jira`, `github`) from the .env file
//! * `GET /api/setup`: what the setup assistant prefills, [`Setup`]
//! * `PUT /api/config`: body [`ConfigChange`], writes `config.toml`, response [`ConfigResponse`]
//! * `PUT /api/secrets/{name}`: body [`KeyInput`], stores the key in the .env file,
//!   response [`KeyResponse`] (only „set“, never the value)
//! * `GET /api/agent/image` / `POST /api/agent/image`: state or start of „build image“, [`ImageBuild`]
//! * `GET /api/agent/check/container`: the same check from a container, [`ContainerCheck`]
//! * `GET /api/agent/pull-request?run=<key>`: title and text of the pull request that would be created for the
//!   branch of a finished real run, and the one that already exists for it, [`PullRequestPreview`]
//! * `GET /api/health`: under `agent` an [`AgentStatus`], under `endpoint` an [`EndpointStatus`], under `container` a [`ContainerStatus`]
//!
//! Both sides use these types so that the shapes do not drift apart.

use serde::{Deserialize, Serialize};

use crate::{Action, AgentRepo, AgentRepos, LogEntry, PullRequestRef};

/// Body of `POST /api/actions`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionRequest {
    pub action: Action,
}

/// Response to an applied action.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionResponse {
    /// Revision of the state after the action.
    pub revision: u64,
    /// The sentence from the log (see `Applied::summary`).
    pub summary: String,
    /// Number of new log entries.
    pub logged: usize,
}

/// Response to `POST /api/sync` („sync now“).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SyncResponse {
    /// `false` if no source is set up (Jira or GitHub with key).
    pub active: bool,
    /// Revision of the state after the sync.
    pub revision: u64,
    /// Number of new log entries.
    pub logged: usize,
}

/// A Jira project for the selection in the inbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JiraProject {
    /// Key, e.g. `BAU`.
    pub key: String,
    /// Name, e.g. „Baukasten“.
    #[serde(default)]
    pub name: String,
}

/// Response to `GET /api/jira/projects`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraProjectsResponse {
    /// `false` if Jira is not set up or the key is missing.
    pub active: bool,
    #[serde(default)]
    pub projects: Vec<JiraProject>,
}

/// What the connection test to Jira failed on (or `Ok`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JiraCause {
    /// Jira responds and knows the token.
    Ok,
    /// Address, email or token are still missing.
    #[default]
    Details,
    /// No Jira responds at the address (404 or another page).
    Address,
    /// Jira rejects email or token (401).
    Token,
    /// The token is valid but does not have the necessary rights (403).
    Permission,
    /// No connection (network, VPN, address not found).
    Reachable,
    /// Something else (quota, Jira outage).
    Other,
}

/// Response to `POST /api/jira/check`: read only (`GET /rest/api/3/myself`), never the token.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraCheck {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub cause: JiraCause,
    /// Plain text for the UI.
    #[serde(default)]
    pub message: String,
    /// On success: the display name under which Jira knows you.
    #[serde(default)]
    pub name: Option<String>,
}

/// Response to `POST /api/oauth/{service}/revoke`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RevokeResponse {
    /// The tokens in the .env file are deleted.
    pub disconnected: bool,
    /// The service itself revoked the token (not all offer that).
    #[serde(default)]
    pub at_service: bool,
    /// What the user can do otherwise or why the service did not revoke.
    #[serde(default)]
    pub hint: Option<String>,
}

/// What the agent works with: the tool that runs in the container. So far only the
/// Claude Agent SDK is implemented; the others are known values that report „not implemented yet“ in plain text.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[default]
    ClaudeAgentSdk,
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 3] = [
        Provider::ClaudeAgentSdk,
        Provider::ClaudeCode,
        Provider::Codex,
    ];

    /// The name in `config.toml` (`[agent] provider`) and in the JSON.
    pub fn id(self) -> &'static str {
        match self {
            Provider::ClaudeAgentSdk => "claude-agent-sdk",
            Provider::ClaudeCode => "claude-code",
            Provider::Codex => "codex",
        }
    }

    pub fn from_id(text: &str) -> Option<Provider> {
        let t = text.trim().to_lowercase();
        Self::ALL.into_iter().find(|a| a.id() == t)
    }

    /// The name for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Provider::ClaudeAgentSdk => "Claude Agent SDK",
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    /// Whether the runner can already execute this tool.
    pub fn implemented(self) -> bool {
        self == Provider::ClaudeAgentSdk
    }

    /// Plain text for the case that a run would need this tool but it is missing.
    pub fn not_implemented(self) -> Option<String> {
        (!self.implemented()).then(|| {
            format!(
                "{} ist noch nicht umgesetzt. Wähle unter „Einrichten“ › „Modell“ das Claude Agent SDK.",
                self.label()
            )
        })
    }
}

/// The agent's connection to its model, as `GET /api/health` reports it. Never the key itself,
/// only whether one is in the .env file.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointStatus {
    /// `[agent.endpoint]` (or the older `[litellm]`) is in `config.toml`.
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub provider: Provider,
    /// Address of the endpoint; empty = directly at the provider.
    #[serde(default)]
    pub url: Option<String>,
    /// The model name.
    #[serde(default)]
    pub model: Option<String>,
    /// A key is in the .env file (`agent-key`, alternatively the older `litellm`).
    #[serde(default)]
    pub key: bool,
}

/// The container runtime, as `GET /api/health` reports it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerStatus {
    /// `docker` or `podman`; `None` if none was found or chosen.
    #[serde(default)]
    pub runtime: Option<String>,
    /// `<cli> info` succeeded: the engine is running.
    #[serde(default)]
    pub engine_running: bool,
    /// Where the choice comes from: `config`, `env`, `detected` or `none`.
    #[serde(default)]
    pub source: String,
    /// What to do if it does not work.
    #[serde(default)]
    pub hint: Option<String>,
}

/// Whether the real agent can start, as `GET /api/health` reports it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStatus {
    /// `echt` (container) or `simuliert` (seed `demo`, only for trying out).
    #[serde(default)]
    pub mode: String,
    /// Runs can start: everything necessary is set up (always true in mode `simuliert`).
    #[serde(default)]
    pub ready: bool,
    /// The agent image is built (`image inspect`).
    #[serde(default)]
    pub image: bool,
    /// Name of the image, e.g. `localhost/cockpit-agent:4`.
    #[serde(default)]
    pub image_name: String,
    /// The default repo of the agent (`owner/name`), if there is one.
    #[serde(default)]
    pub repo: Option<String>,
    /// The repos the agent may work in, with base branch and the Jira projects routed to each,
    /// and the default. The interface preselects the repo with the same rules the backend applies.
    #[serde(default)]
    pub repos: AgentRepos,
    /// What is still missing, one sentence per entry in plain text with the place in the assistant.
    #[serde(default)]
    pub missing: Vec<String>,
}

/// State of „build image“ (`GET`/`POST /api/agent/image`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageBuild {
    /// The build is running right now.
    #[serde(default)]
    pub running: bool,
    /// The image exists (after the last build or already before).
    #[serde(default)]
    pub present: bool,
    /// Why the last build failed, in plain text.
    #[serde(default)]
    pub error: Option<String>,
    /// The last lines of the build output.
    #[serde(default)]
    pub lines: Vec<String>,
}

/// Response to `GET /api/agent/check/container`: does a container reach the endpoint?
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerCheck {
    /// It could be checked (image, runtime and endpoint are there).
    #[serde(default)]
    pub checked: bool,
    /// The Mac (the backend) reaches the endpoint.
    #[serde(default)]
    pub mac_reached: bool,
    /// The container reaches it.
    #[serde(default)]
    pub container_reached: bool,
    /// The endpoint accepts the key in the container.
    #[serde(default)]
    pub key_ok: bool,
    /// What to do next or what the cause was.
    #[serde(default)]
    pub hint: Option<String>,
}

/// Response to `GET /api/agent/check`: what the connection test found. Never contains the key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointCheck {
    /// There is something to check (address or model entered); otherwise nothing was called.
    #[serde(default)]
    pub configured: bool,
    /// The endpoint answered `GET <url>/v1/models`.
    #[serde(default)]
    pub reachable: bool,
    /// The endpoint accepted the key.
    #[serde(default)]
    pub key_ok: bool,
    /// The configured model name is in the endpoint's list.
    #[serde(default)]
    pub model_present: bool,
    /// What to do next or what the cause was.
    #[serde(default)]
    pub hint: Option<String>,
}

/// Response to `GET /api/agent/models`: the model IDs of the endpoint (IDs only, Claude models
/// first, at most [`MAX_MODELS`]). Never contains the key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelList {
    /// The list could be loaded.
    #[serde(default)]
    pub loaded: bool,
    #[serde(default)]
    pub reachable: bool,
    #[serde(default)]
    pub key_ok: bool,
    #[serde(default)]
    pub models: Vec<String>,
    /// Plain text if something did not work.
    #[serde(default)]
    pub hint: Option<String>,
}

/// The list delivers at most this many model IDs.
pub const MAX_MODELS: usize = 200;

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

/// Jira details for `config.toml`. `None` leaves the value unchanged, an empty text removes it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraInput {
    #[serde(default)]
    pub site: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub projects: Option<Vec<String>>,
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

/// The repos the agent works in, and the rules that route tickets to them. Replaces the whole
/// list (`[[agent.repos]]`) and the default (`[agent] repo`); an empty list removes both.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentReposInput {
    #[serde(default)]
    pub repos: Vec<AgentRepo>,
    /// Must be one of `repos`; `None` = no default (the user chooses at every start).
    #[serde(default)]
    pub default: Option<String>,
}

/// What a read-only look at a repo found.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoState {
    /// It exists and the key may push (what the agent needs).
    Ok,
    /// It exists and the key may read, but not push.
    ReadOnly,
    /// No such repo, or the key cannot see it.
    NotFound,
    /// The key is missing, expired or lacks the permission.
    NoAccess,
    /// The check did not work (network, quota …).
    Failed,
}

/// Result of checking one repo (`POST /api/github/repos/check`). Only reads.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RepoCheck {
    pub repo: String,
    pub state: RepoState,
    /// The repo's own default branch, if known.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// A sentence in plain text for the assistant.
    #[serde(default)]
    pub message: String,
}

/// Body of `POST /api/github/repos/check`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoCheckRequest {
    #[serde(default)]
    pub repos: Vec<String>,
}

/// Response of `POST /api/github/repos/check`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoCheckResponse {
    #[serde(default)]
    pub repos: Vec<RepoCheck>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GitHubInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_repos: Option<AgentReposInput>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "litellm")]
    pub endpoint: Option<EndpointInput>,
    /// `[server] webhooks` and `[server] public_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhooks: Option<WebhooksInput>,
}

/// Switch and public address for webhooks. `None` leaves a value unchanged; an empty address
/// removes it. The address is only shown to the user (for the two target URLs), never called.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhooksInput {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub public_url: Option<String>,
}

/// What the assistant shows about one webhook source. Never the secret.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookSourceInfo {
    /// Path of the route on this server, e.g. `/api/webhooks/github`.
    #[serde(default)]
    pub path: String,
    /// A secret is in the `.env` file.
    #[serde(default)]
    pub secret_set: bool,
    /// When a correctly signed request last arrived (milliseconds since 1970); memory only.
    #[serde(default)]
    pub last_received_ms: Option<u64>,
    /// Its event type (`ping`, `pull_request`, `jira:issue_updated`, ...), nothing else of it.
    #[serde(default)]
    pub last_event: Option<String>,
}

/// State of the optional webhooks (`GET /api/setup`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhooksInfo {
    #[serde(default)]
    pub enabled: bool,
    /// `[server] public_url`: the address of the tunnel, for display only.
    #[serde(default)]
    pub public_url: Option<String>,
    /// Poll interval of a source while its webhooks arrive regularly.
    #[serde(default)]
    pub fallback_interval_s: u64,
    #[serde(default)]
    pub github: WebhookSourceInfo,
    #[serde(default)]
    pub jira: WebhookSourceInfo,
}

/// Response of `POST /api/setup/webhook-secret/{source}`: the only time the value is sent.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookSecretResponse {
    #[serde(default)]
    pub secret: String,
}

/// Response of `POST /api/setup/webhook-test/{source}`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookTestResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub message: String,
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
    #[serde(default, alias = "litellm_url")]
    pub endpoint_url: Option<String>,
    #[serde(default, alias = "litellm_modell")]
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
}

impl Setup {
    /// Is the key `name` in the .env file?
    pub fn has_key(&self, name: &str) -> bool {
        let da = |n: &str| self.keys.iter().any(|s| s.name == n && s.set);
        // The endpoint's key was called `litellm` until #31 and still applies.
        da(name) || (name == "agent-key" && da("litellm"))
    }
}

/// Error response (4xx/5xx) with German message for the UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ErrorBody {
    pub error: String,
}

/// An event on `GET /api/events`.
///
/// After every applied action one arrives with the new log entries. Right
/// after connecting one arrives with an empty list and the current revision. Whoever sees a
/// gap in the revisions (or was connected and lost the connection) reloads
/// the state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ServerEvent {
    pub revision: u64,
    #[serde(default)]
    pub entries: Vec<LogEntry>,
}

#[cfg(test)]
mod tests;

/// Preview of the pull request for the branch of a finished real run
/// (`GET /api/agent/pull-request?run=<key>`). Nothing is created by asking for it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PullRequestPreview {
    /// `owner/name`
    #[serde(default)]
    pub repo: String,
    /// The branch of the agent.
    #[serde(default)]
    pub head: String,
    /// The branch the pull request goes against (from the configuration of the run).
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// It would be created as a draft (the option of the run). Making it „ready“ is up to you on GitHub.
    #[serde(default)]
    pub draft: bool,
    /// GitHub already has an open pull request for the branch: it is linked instead of creating one.
    #[serde(default)]
    pub existing: Option<PullRequestRef>,
}
