//! „Aufräumen“ and „Branch löschen“ (`/api/agent/cleanup`, `/api/agent/branch`).

use cockpit_core::api::{
    BranchCheck, BranchDeleteRequest, BranchDeleted, CleanupPreview, CleanupRequest, CleanupResult,
};
use gloo_net::http::Request;

use super::{CallError, rejection};

fn unreadable() -> CallError {
    CallError::Rejected(feature_ui::t!(ServerGarbled))
}

/// What „Aufräumen“ would remove (`GET /api/agent/cleanup`). Removes nothing.
pub async fn fetch_cleanup_preview() -> Result<CleanupPreview, CallError> {
    let resp = Request::get("/api/agent/cleanup")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json().await.map_err(|_| unreadable())
}

/// Removes what the user confirmed (`POST /api/agent/cleanup`).
pub async fn post_cleanup(request: &CleanupRequest) -> Result<CleanupResult, CallError> {
    let resp = Request::post("/api/agent/cleanup")
        .json(request)
        .map_err(|_| CallError::Rejected(feature_ui::t!(ServerRequestNotStarted)))?
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json().await.map_err(|_| unreadable())
}

/// May the branch of the run be deleted (`GET /api/agent/branch?run=…`)? Reads only.
pub async fn fetch_branch_check(run: &str) -> Result<BranchCheck, CallError> {
    let resp = Request::get("/api/agent/branch")
        .query([("run", run)])
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json().await.map_err(|_| unreadable())
}

/// Deletes the branch of the run on GitHub (`POST /api/agent/branch`), only on a click.
pub async fn post_branch_delete(run: &str) -> Result<BranchDeleted, CallError> {
    let resp = Request::post("/api/agent/branch")
        .json(&BranchDeleteRequest { run: run.into() })
        .map_err(|_| CallError::Rejected(feature_ui::t!(ServerRequestNotStarted)))?
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json().await.map_err(|_| unreadable())
}
