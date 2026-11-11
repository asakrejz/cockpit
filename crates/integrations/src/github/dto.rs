//! Antworten der GitHub-REST-API – nur die Felder, die das Cockpit braucht.

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
    /// `owner/name` aus `repository_url`.
    pub fn repo(&self) -> Option<String> {
        let mut teile = self.repository_url.trim_end_matches('/').rsplit('/');
        let name = teile.next()?;
        let owner = teile.next()?;
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
}

/// `GET /repos/{owner}/{repo}/pulls/{number}/files`
#[derive(Debug, Clone, Deserialize)]
pub struct PullFile {
    pub filename: String,
    /// Fehlt bei Binärdateien und sehr großen Änderungen.
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

/// `GET /repos/{owner}/{repo}/pulls/{number}/comments` (Kommentare an Diff-Zeilen)
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewComment {
    pub user: User,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub path: String,
    /// `null`, wenn der Kommentar veraltet ist („outdated“).
    #[serde(default)]
    pub line: Option<u32>,
}

/// `GET /repos/{owner}/{repo}/commits/{sha}/check-runs`
#[derive(Debug, Clone, Deserialize)]
pub struct CheckRuns {
    #[serde(default)]
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    #[serde(default)]
    pub name: String,
    /// `queued`, `in_progress`, `completed`
    #[serde(default)]
    pub status: String,
    /// Nur bei `completed`: `success`, `failure`, `neutral`, `cancelled`, `skipped` …
    #[serde(default)]
    pub conclusion: Option<String>,
}
