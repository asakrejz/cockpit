//! The agent: pull request preview, endpoint and container checks, models, image build and
//! repo suggestions (`/api/agent/…`).

use cockpit_core::api::{ContainerCheck, EndpointCheck, ImageBuild, ModelList, RepoSuggestions};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection, rejection_own};

/// Preview of the pull request for the branch of a finished real run
/// (`GET /api/agent/pull-request?run=…`). Creates nothing.
pub async fn fetch_pull_request_preview(
    run: &str,
    repo: &str,
) -> Result<cockpit_core::api::PullRequestPreview, CallError> {
    let resp = Request::get("/api/agent/pull-request")
        .query([("run", run), ("repo", repo)])
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<cockpit_core::api::PullRequestPreview>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Connection test to the endpoint (`GET /api/agent/check`).
pub async fn fetch_endpoint_check() -> Result<EndpointCheck, CallError> {
    let resp = Request::get("/api/agent/check")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<EndpointCheck>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The model IDs of the saved endpoint (`GET /api/agent/models`).
pub async fn fetch_models() -> Result<ModelList, CallError> {
    let resp = Request::get("/api/agent/models")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ModelList>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// State of "Build image" (`GET /api/agent/image`).
pub async fn fetch_image() -> Result<ImageBuild, CallError> {
    let resp = Request::get("/api/agent/image")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ImageBuild>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Starts "Build image" (`POST /api/agent/image`); the build continues in the server.
pub async fn post_image_build() -> Result<ImageBuild, CallError> {
    let request = Request::post("/api/agent/image")
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerBuildNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ImageBuild>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Checks whether a container reaches the endpoint (`GET /api/agent/check/container`).
pub async fn fetch_container_check() -> Result<ContainerCheck, CallError> {
    let resp = Request::get("/api/agent/check/container")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ContainerCheck>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The repos to offer first when the agent starts on a ticket
/// (`GET /api/agent/repo-suggestions?ticket=…`). Only reads.
pub async fn fetch_repo_suggestions(ticket: &str) -> Result<RepoSuggestions, CallError> {
    let url = format!(
        "/api/agent/repo-suggestions?ticket={}",
        js_sys::encode_uri_component(ticket)
    );
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection_own(resp).await);
    }
    resp.json::<RepoSuggestions>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}
