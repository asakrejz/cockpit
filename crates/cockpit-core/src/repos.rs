//! Repos the agent may work in, and the rules that route a ticket to one of them.
//!
//! Configured in `config.toml` (`[agent] repo` as the default, `[[agent.repos]]` with a base
//! branch and the Jira projects that go there). The backend reports the result in
//! `GET /api/health`, so the interface can preselect the repo; the backend applies the same
//! rules when a run starts. Everything here is pure.

use serde::{Deserialize, Serialize};

pub mod suggest;

/// Base branch if none is configured.
pub const DEFAULT_BASE: &str = "main";

fn default_base() -> String {
    DEFAULT_BASE.into()
}

fn default_pattern() -> String {
    crate::branch::DEFAULT_PATTERN.into()
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
    /// How the agent names its branches (`{ticket}`, `{slug}`, `{type}`); see [`crate::branch`].
    #[serde(default = "default_pattern")]
    pub branch_pattern: String,
    /// Language of the pull request title and text (the team language). English by default.
    #[serde(default, skip_serializing_if = "is_english")]
    pub pr_language: cockpit_i18n::Lang,
    /// Does the sync load all open pull requests of this repo (a "watched" repo)? `true` for
    /// every entry written by hand. Entries the app stores on the first use of a repo set it to
    /// `false`: they only keep the settings and do not widen the sync.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub watch: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// English is the default and is not written (older files and snapshots stay as they were).
pub(crate) fn is_english(lang: &cockpit_i18n::Lang) -> bool {
    *lang == cockpit_i18n::Lang::En
}

impl AgentRepo {
    pub fn new(repo: &str) -> Self {
        Self {
            repo: repo.into(),
            base_branch: default_base(),
            projects: Vec::new(),
            branch_pattern: default_pattern(),
            pr_language: cockpit_i18n::Lang::En,
            watch: true,
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

    /// Language of the pull request text of a repo (English if it is unknown or sets none).
    pub fn pr_language(&self, repo: &str) -> cockpit_i18n::Lang {
        self.find(repo).map(|r| r.pr_language).unwrap_or_default()
    }

    /// Branch pattern of a repo (the default pattern if it is unknown or sets none).
    pub fn branch_pattern(&self, repo: &str) -> String {
        self.find(repo)
            .map(|r| r.branch_pattern.trim())
            .filter(|p| !p.is_empty())
            .map_or_else(default_pattern, str::to_string)
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

    /// The one list of the assistant: the configured repos plus those that only the older
    /// `[github] repos` names (they stay, so nothing is lost when the list is saved again).
    ///
    /// ```
    /// use cockpit_core::{AgentRepo, AgentRepos};
    /// let configured = AgentRepos { repos: vec![AgentRepo::new("acme/web")], default: None };
    /// let all = configured.unified(&["acme/app".into(), "acme/web".into()]);
    /// let names: Vec<_> = all.repos.iter().map(|r| r.repo.as_str()).collect();
    /// assert_eq!(names, ["acme/web", "acme/app"]);
    /// ```
    pub fn unified(&self, github: &[String]) -> AgentRepos {
        let mut all = self.clone();
        for r in github {
            if all.find(r).is_none() {
                all.repos.push(AgentRepo::new(r));
            }
        }
        all
    }

    /// Names of the repos, in list order: the repos the sync compares.
    pub fn names(&self) -> Vec<String> {
        self.repos.iter().map(|r| r.repo.clone()).collect()
    }

    /// Checks a repo chosen by hand: it has to be one of the configured ones.
    pub fn allows(&self, repo: &str) -> bool {
        self.find(repo).is_some()
    }
}

#[cfg(test)]
mod tests;
