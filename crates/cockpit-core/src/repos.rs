//! Repos the agent may work in, and the default among them.
//!
//! Everything here is pure.

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
            branch_pattern: default_pattern(),
            pr_language: cockpit_i18n::Lang::En,
            watch: true,
        }
    }
}

/// The repos of the agent and the default one.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRepos {
    #[serde(default)]
    pub repos: Vec<AgentRepo>,
    /// The repo a start uses when the user chooses none.
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

    /// Names of the repos, in list order: the repos the sync compares.
    pub fn names(&self) -> Vec<String> {
        self.repos.iter().map(|r| r.repo.clone()).collect()
    }
}

#[cfg(test)]
mod tests;
