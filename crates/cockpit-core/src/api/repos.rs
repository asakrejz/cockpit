//! Wire shapes of the repository checks.

use super::*;

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
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
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

/// Body of `POST /api/github/branch-suggestion`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchSuggestionRequest {
    #[serde(default)]
    pub repo: String,
}

/// What the contribution guidelines of a repo say about branch names. Only a proposal: the
/// user confirms it, nothing is saved by this call.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchSuggestion {
    /// The proposed pattern, `None` if the guidelines name none.
    #[serde(default)]
    pub pattern: Option<String>,
    /// The file the proposal comes from (`CONTRIBUTING.md`).
    #[serde(default)]
    pub source: Option<String>,
    /// A sentence in plain text: what was found, or why there is no proposal.
    #[serde(default)]
    pub message: String,
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

/// One repo the connected GitHub access can reach (`GET /api/github/repos`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GitHubRepoInfo {
    /// `owner/name`
    pub repo: String,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub archived: bool,
    /// The repo's own default branch; preselects the base branch.
    #[serde(default)]
    pub default_branch: String,
    /// `None` if GitHub does not say.
    #[serde(default)]
    pub can_push: Option<bool>,
}

/// Response of `GET /api/github/repos`. Only reads.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHubRepoList {
    #[serde(default)]
    pub repos: Vec<GitHubRepoInfo>,
    /// There are more repos than were listed (the list stops at 500); use the search of GitHub
    /// or type the name.
    #[serde(default)]
    pub truncated: bool,
    /// Which access produced the list: `token` or `app`.
    #[serde(default)]
    pub via: String,
}

/// What the connection test of GitHub found.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitHubCause {
    Ok,
    /// No token / sign-in yet.
    NotConnected,
    /// 401: GitHub rejects the token.
    Token,
    /// 403: valid, but without permission.
    Permission,
    /// The quota is used up.
    RateLimit,
    /// No connection to GitHub.
    Network,
    Other,
}

/// Response of `POST /api/github/check`: who GitHub sees, and per repo what the key may do.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GitHubCheck {
    pub ok: bool,
    pub cause: GitHubCause,
    /// A sentence in plain text. Never names the token.
    pub message: String,
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
    /// Login of the key holder.
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub repos: Vec<RepoCheck>,
}

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
