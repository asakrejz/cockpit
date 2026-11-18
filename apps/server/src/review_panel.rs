//! Review panel, backend part: the commits of a pull request and the diff of one commit, read
//! live from GitHub with the token of the person (`GET /api/review/commits`, `GET /api/review/commit`).
//! Read only. Only pull requests the app already knows can be asked about, so the routes cannot be
//! used to read an arbitrary repo.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cockpit_core::PrKey;
use cockpit_core::api::{PanelCommitDiff, PanelCommits};
use cockpit_integrations::github::{panel_commits, panel_diff};
use serde::Deserialize;

use crate::agent_followup::github_message;
use cockpit_i18n::{Msg, msg};

use crate::app::AppState;
use crate::app::error_msg;

#[derive(Deserialize)]
pub(crate) struct PanelQuery {
    repo: String,
    number: u32,
    sha: Option<String>,
    path: Option<String>,
}

/// A commit ID as it may appear in a URL path: 7 to 40 hexadecimal characters.
fn valid_sha(sha: &str) -> bool {
    (7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit())
}

impl AppState {
    /// The pull request must be in the synced state.
    fn panel_known(&self, repo: &str, number: u32) -> Result<(), Msg> {
        let known = self
            .inner
            .live
            .copy()
            .review(PrKey::new(repo, number))
            .is_some();
        if known {
            Ok(())
        } else {
            Err(msg!(
                ServerAgentPanelUnknownPr,
                repo = repo,
                number = number
            ))
        }
    }

    pub(crate) async fn panel_commits(&self, repo: &str, number: u32) -> Result<PanelCommits, Msg> {
        self.panel_known(repo, number)?;
        let client = self.github_client_async().await?;
        let list = client
            .commits(repo, number)
            .await
            .map_err(|e| github_message(msg!(ServerAgentWhatCommitsRead), &e))?;
        Ok(panel_commits(&list))
    }

    pub(crate) async fn panel_commit_diff(
        &self,
        repo: &str,
        number: u32,
        sha: &str,
        path: Option<&str>,
    ) -> Result<PanelCommitDiff, Msg> {
        self.panel_known(repo, number)?;
        let client = self.github_client_async().await?;
        let detail = client
            .commit(repo, sha)
            .await
            .map_err(|e| github_message(msg!(ServerAgentWhatCommitRead), &e))?;
        Ok(panel_diff(&detail, path))
    }
}

/// `GET /api/review/commits?repo=<owner/name>&number=<n>`
pub(crate) async fn commits(
    State(state): State<AppState>,
    Query(q): Query<PanelQuery>,
) -> Response {
    match state.panel_commits(&q.repo, q.number).await {
        Ok(c) => Json(c).into_response(),
        Err(m) => error_msg(StatusCode::BAD_GATEWAY, m),
    }
}

/// `GET /api/review/commit?repo=…&number=…&sha=…[&path=…]`
pub(crate) async fn commit(State(state): State<AppState>, Query(q): Query<PanelQuery>) -> Response {
    let Some(sha) = q.sha.as_deref().filter(|s| valid_sha(s)) else {
        return error_msg(StatusCode::BAD_REQUEST, msg!(ServerAgentShaInvalid));
    };
    match state
        .panel_commit_diff(&q.repo, q.number, sha, q.path.as_deref())
        .await
    {
        Ok(d) => Json(d).into_response(),
        Err(m) => error_msg(StatusCode::BAD_GATEWAY, m),
    }
}
