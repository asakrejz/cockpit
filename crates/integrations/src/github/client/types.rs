//! What the GitHub client takes and returns: the plain types around its calls.

use super::super::dto::{CheckRun, Pull, RepoListEntry};

/// Why a pull request is in the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// A review from me is requested.
    ReviewRequested,
    /// The pull request is mine.
    Own,
}

impl Relation {
    pub(super) fn search(self) -> &'static str {
        match self {
            Relation::ReviewRequested => "is:pr is:open archived:false review-requested:@me",
            Relation::Own => "is:pr is:open archived:false author:@me",
        }
    }
}

/// What a key may do in a repo (see [`GitHubClient::repo_access`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoAccess {
    pub default_branch: String,
    pub archived: bool,
    /// `None` if the service does not say.
    pub can_push: Option<bool>,
}

/// One repo of the list the user can choose from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoInfo {
    /// `owner/name`
    pub repo: String,
    pub private: bool,
    pub archived: bool,
    pub default_branch: String,
    /// `None` if the service does not say.
    pub can_push: Option<bool>,
}

/// The repos a key can reach. `truncated`: there are more than [`MAX_REPOS`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoList {
    pub repos: Vec<RepoInfo>,
    pub truncated: bool,
}

/// At most this many repos are listed.
pub const MAX_REPOS: usize = 500;

impl From<RepoListEntry> for RepoInfo {
    fn from(r: RepoListEntry) -> Self {
        Self {
            repo: r.full_name,
            private: r.private,
            archived: r.archived,
            default_branch: r
                .default_branch
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".into()),
            can_push: r.permissions.map(|p| p.push),
        }
    }
}

/// How many open pull requests the sync loads at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrLimits {
    /// Per repo (the most recently updated ones come first).
    pub per_repo: usize,
    /// Over all repos together.
    pub total: usize,
}

impl PrLimits {
    pub const DEFAULT_PER_REPO: usize = 200;
    pub const DEFAULT_TOTAL: usize = 500;
}

impl Default for PrLimits {
    fn default() -> Self {
        Self {
            per_repo: Self::DEFAULT_PER_REPO,
            total: Self::DEFAULT_TOTAL,
        }
    }
}

/// The open pull requests of one repo.
#[derive(Clone, Debug)]
pub struct OpenPulls {
    pub pulls: Vec<Pull>,
    /// There are more open pull requests than were loaded.
    pub truncated: bool,
}

/// What the sync knows about the checks of one commit.
#[derive(Clone, Debug)]
pub struct LoadedChecks {
    /// The individual runs. Empty when only the commit status was readable.
    pub runs: Vec<CheckRun>,
    /// The overall state; `Unknown` when neither the check runs nor the commit status are readable.
    pub checks: cockpit_core::Checks,
}
