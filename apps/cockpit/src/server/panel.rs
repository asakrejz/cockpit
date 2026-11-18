//! Review panel: commits of a pull request and the diff of one commit (`/api/review/…`).

use cockpit_core::api::{PanelCommitDiff, PanelCommits};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection};

async fn get_json<T: serde::de::DeserializeOwned>(
    path: &str,
    query: Vec<(&str, String)>,
) -> Result<T, CallError> {
    let resp = Request::get(path)
        .query(query.iter().map(|(k, v)| (*k, v.as_str())))
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<T>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The commits of a pull request (`GET /api/review/commits`).
pub async fn fetch_panel_commits(repo: &str, number: u32) -> Result<PanelCommits, CallError> {
    get_json(
        "/api/review/commits",
        vec![("repo", repo.into()), ("number", number.to_string())],
    )
    .await
}

/// The diff of one commit (`GET /api/review/commit`); with `path` only that file, in full.
pub async fn fetch_panel_commit(
    repo: &str,
    number: u32,
    sha: &str,
    path: Option<&str>,
) -> Result<PanelCommitDiff, CallError> {
    let mut query = vec![
        ("repo", repo.to_string()),
        ("number", number.to_string()),
        ("sha", sha.to_string()),
    ];
    if let Some(path) = path {
        query.push(("path", path.to_string()));
    }
    get_json("/api/review/commit", query).await
}
