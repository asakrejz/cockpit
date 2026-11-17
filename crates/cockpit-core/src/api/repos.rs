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
