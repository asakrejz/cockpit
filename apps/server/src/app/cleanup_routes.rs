//! The HTTP side of "Clean up" and "Delete branch" (see `cockpit_core::api::cleanup`).

use cockpit_core::api::{BranchDeleteRequest, CleanupRequest};
use cockpit_i18n::msg;

use super::*;

/// `GET /api/agent/cleanup`: what would be removed. Removes nothing.
pub(super) async fn cleanup_preview(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.cleanup_preview()).await {
        Ok(p) => Json(p).into_response(),
        Err(_) => error_msg(StatusCode::INTERNAL_SERVER_ERROR, msg!(ServerAgentInternal)),
    }
}

/// `POST /api/agent/cleanup`: removes what the body confirms and what is still an orphan.
pub(super) async fn cleanup_apply(
    State(state): State<AppState>,
    Json(request): Json<CleanupRequest>,
) -> Response {
    match tokio::task::spawn_blocking(move || state.cleanup_apply(&request)).await {
        Ok(r) => Json(r).into_response(),
        Err(_) => error_msg(StatusCode::INTERNAL_SERVER_ERROR, msg!(ServerAgentInternal)),
    }
}

/// `GET /api/agent/branch?run=<key>`: may the branch of the run be deleted? Reads only.
pub(super) async fn branch_check(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(run) = q.get("run").filter(|r| !r.is_empty()) else {
        return error_msg(StatusCode::BAD_REQUEST, msg!(ServerAgentRunMissing));
    };
    Json(state.branch_check(run).await).into_response()
}

/// `POST /api/agent/branch`: deletes the branch of the run – only after the same check.
pub(super) async fn branch_delete(
    State(state): State<AppState>,
    Json(request): Json<BranchDeleteRequest>,
) -> Response {
    match state.branch_delete(&request.run).await {
        Ok(d) => Json(d).into_response(),
        Err(e) => action_error(e),
    }
}
