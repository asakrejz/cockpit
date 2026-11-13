//! Responses of the GitHub REST API – only the fields the cockpit needs.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub login: String,
}

/// `GET /search/issues`
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub items: Vec<SearchItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchItem {
    pub number: u32,
    /// `https://api.github.com/repos/<owner>/<name>`
    pub repository_url: String,
}

impl SearchItem {
    /// `owner/name` from `repository_url`.
    pub fn repo(&self) -> Option<String> {
        let mut parts = self.repository_url.trim_end_matches('/').rsplit('/');
        let name = parts.next()?;
        let owner = parts.next()?;
        (!name.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{name}"))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitRef {
    #[serde(rename = "ref")]
    pub name: String,
    #[serde(default)]
    pub sha: String,
}

/// `GET /repos/{owner}/{repo}/pulls/{number}`
#[derive(Debug, Clone, Deserialize)]
pub struct Pull {
    pub number: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    pub user: User,
    pub head: GitRef,
    pub base: GitRef,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
}

/// `GET /repos/{owner}/{repo}/pulls/{number}/files`
#[derive(Debug, Clone, Deserialize)]
pub struct PullFile {
    pub filename: String,
    /// Missing for binary files and very large changes.
    #[serde(default)]
    pub patch: Option<String>,
}

/// `GET /repos/{owner}/{repo}/issues/{number}/comments`
#[derive(Debug, Clone, Deserialize)]
pub struct IssueComment {
    pub user: User,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

/// `GET /repos/{owner}/{repo}/pulls/{number}/comments` (comments on diff lines)
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewComment {
    #[serde(default)]
    pub id: u64,
    /// The comment this one answers (threads hang on their first comment).
    #[serde(default)]
    pub in_reply_to_id: Option<u64>,
    pub user: User,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub path: String,
    /// `null` when the comment is outdated („outdated“).
    #[serde(default)]
    pub line: Option<u32>,
}

/// `GET /repos/{owner}/{repo}/commits/{sha}/check-runs`
#[derive(Debug, Clone, Deserialize)]
pub struct CheckRuns {
    #[serde(default)]
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CheckRun {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    /// `queued`, `in_progress`, `completed`
    #[serde(default)]
    pub status: String,
    /// Only for `completed`: `success`, `failure`, `neutral`, `cancelled`, `skipped` …
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub output: Option<CheckOutput>,
}

impl CheckRun {
    /// Output of the run (title and summary), as far as GitHub provides it.
    pub fn log(&self) -> String {
        let Some(out) = &self.output else {
            return String::new();
        };
        [out.title.as_deref(), out.summary.as_deref()]
            .into_iter()
            .flatten()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CheckOutput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

/// `GET /repos/{owner}/{repo}/check-runs/{id}/annotations`
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Annotation {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub start_line: u32,
    /// `notice`, `warning`, `failure`
    #[serde(default)]
    pub annotation_level: String,
    #[serde(default)]
    pub message: String,
}

/// `GET /repos/{owner}/{repo}/pulls?head=…` and `POST /repos/{owner}/{repo}/pulls`
#[derive(Debug, Clone, Deserialize)]
pub struct PullSummary {
    pub number: u32,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub head: Option<GitRef>,
}

/// `GET /repos/{owner}/{repo}/pulls/{number}/reviews`
#[derive(Debug, Clone, Deserialize)]
pub struct PullReview {
    pub user: User,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`, `PENDING`
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub submitted_at: String,
}

/// `GET /repos/{owner}/{repo}`
#[derive(Debug, Clone, Deserialize)]
pub struct Repo {
    pub default_branch: String,
    #[serde(default)]
    pub archived: bool,
    /// What the key holder may do in the repo (missing for some kinds of keys).
    #[serde(default)]
    pub permissions: Option<RepoPermissions>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RepoPermissions {
    #[serde(default)]
    pub push: bool,
}

/// `GET /repos/{owner}/{repo}/git/ref/heads/{branch}`
#[derive(Debug, Clone, Deserialize)]
pub struct GitRefResponse {
    pub object: GitObject,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitObject {
    pub sha: String,
}
