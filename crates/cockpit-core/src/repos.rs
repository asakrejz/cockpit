//! Repos the agent may work in, and the rules that route a ticket to one of them.
//!
//! Configured in `config.toml` (`[agent] repo` as the default, `[[agent.repos]]` with a base
//! branch and the Jira projects that go there). The backend reports the result in
//! `GET /api/health`, so the interface can preselect the repo; the backend applies the same
//! rules when a run starts. Everything here is pure.

use serde::{Deserialize, Serialize};

/// Base branch if none is configured.
pub const DEFAULT_BASE: &str = "main";

fn default_base() -> String {
    DEFAULT_BASE.into()
}

/// One repo the agent may work in.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentRepo {
    /// `owner/name`
    pub repo: String,
    /// Branch the agent branches off and the pull request targets.
    #[serde(default = "default_base")]
    pub base_branch: String,
    /// Jira projects (`WEB`) whose tickets go to this repo.
    #[serde(default)]
    pub projects: Vec<String>,
}

impl AgentRepo {
    pub fn new(repo: &str) -> Self {
        Self {
            repo: repo.into(),
            base_branch: default_base(),
            projects: Vec::new(),
        }
    }
}

/// Why a repo was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoChoice {
    /// A rule matched the project of the ticket.
    Rule(String),
    /// No rule matched; the default repo applies.
    Default(String),
    /// No rule matched and there is no default: the user has to choose.
    None,
}

impl RepoChoice {
    pub fn repo(&self) -> Option<&str> {
        match self {
            RepoChoice::Rule(r) | RepoChoice::Default(r) => Some(r),
            RepoChoice::None => None,
        }
    }
}

/// The repos of the agent and the routing between tickets and repos.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRepos {
    #[serde(default)]
    pub repos: Vec<AgentRepo>,
    /// The repo for tickets that no rule matches.
    #[serde(default)]
    pub default: Option<String>,
}

impl AgentRepos {
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
    }

    pub fn find(&self, repo: &str) -> Option<&AgentRepo> {
        self.repos.iter().find(|r| r.repo == repo)
    }

    /// Base branch of a repo (`main` if it is unknown or sets none).
    pub fn base_branch(&self, repo: &str) -> String {
        self.find(repo)
            .map_or_else(default_base, |r| r.base_branch.clone())
    }

    /// The repo for a ticket of the Jira project `project`: the first rule that names the
    /// project (case does not matter), otherwise the default.
    ///
    /// ```
    /// use cockpit_core::{AgentRepo, AgentRepos, RepoChoice};
    /// let repos = AgentRepos {
    ///     repos: vec![
    ///         AgentRepo::new("acme/app"),
    ///         AgentRepo { projects: vec!["WEB".into()], ..AgentRepo::new("acme/web") },
    ///     ],
    ///     default: Some("acme/app".into()),
    /// };
    /// assert_eq!(repos.choose(Some("WEB")), RepoChoice::Rule("acme/web".into()));
    /// assert_eq!(repos.choose(Some("BAU")), RepoChoice::Default("acme/app".into()));
    /// ```
    pub fn choose(&self, project: Option<&str>) -> RepoChoice {
        if let Some(p) = project
            && let Some(r) = self
                .repos
                .iter()
                .find(|r| r.projects.iter().any(|q| q.eq_ignore_ascii_case(p)))
        {
            return RepoChoice::Rule(r.repo.clone());
        }
        match &self.default {
            Some(d) => RepoChoice::Default(d.clone()),
            None => RepoChoice::None,
        }
    }

    /// Checks a repo chosen by hand: it has to be one of the configured ones.
    pub fn allows(&self, repo: &str) -> bool {
        self.find(repo).is_some()
    }
}

#[cfg(test)]
mod tests;
