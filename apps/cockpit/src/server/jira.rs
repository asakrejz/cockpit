//! Jira: projects, assignable people and the connection test (`/api/jira/…`).

use cockpit_core::api::{AssignableResponse, JiraCheck, JiraProjectsResponse};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection};

/// The Jira projects for the selection in the inbox (`GET /api/jira/projects`).
pub async fn fetch_jira_projects() -> Result<JiraProjectsResponse, CallError> {
    let resp = Request::get("/api/jira/projects")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<JiraProjectsResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// People Jira lets you assign tickets of `project` to, matching `query`
/// (`GET /api/jira/assignable`, read only).
pub async fn fetch_jira_assignable(
    project: &str,
    query: &str,
) -> Result<AssignableResponse, CallError> {
    let resp = Request::get("/api/jira/assignable")
        .query([("project", project), ("query", query)])
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<AssignableResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Connection test for Jira via API token (`POST /api/jira/check`). Read only; the answer
/// never contains the token.
pub async fn post_jira_check() -> Result<JiraCheck, CallError> {
    let request = Request::post("/api/jira/check")
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerTestNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<JiraCheck>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}
