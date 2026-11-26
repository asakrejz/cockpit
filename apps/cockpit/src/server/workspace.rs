//! The workspace itself: loading it, sending actions, "Sync now" (`/api/workspace`,
//! `/api/actions`, `/api/sync`).

use cockpit_core::api::{ActionRequest, ActionResponse, SyncResponse};
use cockpit_core::{Action, Workspace, persist};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection};

/// Loads the current state including the revision.
pub async fn fetch_workspace() -> Result<(Workspace, u64), CallError> {
    let resp = Request::get("/api/workspace")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    let text = resp.text().await.map_err(|_| CallError::Unreachable)?;
    persist::load_with_revision(&text).map_err(|_| CallError::Rejected(t!(ServerUnreadableState)))
}

/// Sends an action. The server applies it as "Du".
pub async fn post_action(action: Action) -> Result<ActionResponse, CallError> {
    let request = Request::post("/api/actions")
        .header("Content-Type", "application/json")
        .json(&ActionRequest { action })
        .map_err(|_| CallError::Rejected(t!(ServerActionNotSent)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ActionResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// "Sync now": triggers the sync with Jira and GitHub (`POST /api/sync`).
pub async fn post_sync() -> Result<SyncResponse, CallError> {
    let request = Request::post("/api/sync")
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerSyncNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<SyncResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}
