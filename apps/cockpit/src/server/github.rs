//! GitHub: access, repos, projects, assignable people, branch suggestion and the connection
//! test (`/api/github/…`).

use cockpit_core::api::{
    AssignableResponse, BranchSuggestion, BranchSuggestionRequest, GitHubCheck, GitHubRepoList,
    RepoCheckRequest,
};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection, rejection_own};

/// People GitHub lets you assign issues of `repo` to, matching `query`
/// (`GET /api/github/assignable`, read only).
pub async fn fetch_github_assignable(
    repo: &str,
    query: &str,
) -> Result<AssignableResponse, CallError> {
    let resp = Request::get("/api/github/assignable")
        .query([("repo", repo), ("query", query)])
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

/// Can the access see every configured repo? (`GET /api/github/access`, read-only)
pub async fn fetch_github_access() -> Result<cockpit_core::api::GitHubAccess, CallError> {
    let resp = Request::get("/api/github/access")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<cockpit_core::api::GitHubAccess>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The repos the connected GitHub access can reach (`GET /api/github/repos`). Only reads.
pub async fn fetch_github_repos() -> Result<GitHubRepoList, CallError> {
    let resp = Request::get("/api/github/repos")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection_own(resp).await);
    }
    resp.json::<GitHubRepoList>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The GitHub Projects the connected access can see (`GET /api/github/projects`). Only reads.
pub async fn fetch_github_projects() -> Result<cockpit_core::api::ProjectsResponse, CallError> {
    let resp = Request::get("/api/github/projects")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection_own(resp).await);
    }
    resp.json::<cockpit_core::api::ProjectsResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Creates a GitHub project (`POST /api/github/projects/create`). Only for the click "Create
/// project"; adds things at GitHub, never deletes.
pub async fn post_project_create(
    request: &cockpit_core::api::CreateProjectRequest,
) -> Result<cockpit_core::api::CreateProjectResponse, CallError> {
    let resp = Request::post("/api/github/projects/create")
        .json(request)
        .map_err(|_| CallError::Unreachable)?
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection_own(resp).await);
    }
    resp.json::<cockpit_core::api::CreateProjectResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// A branch pattern proposed from the CONTRIBUTING.md of a repo
/// (`POST /api/github/branch-suggestion`). Only reads; nothing is saved.
pub async fn post_branch_suggestion(repo: &str) -> Result<BranchSuggestion, CallError> {
    let request = Request::post("/api/github/branch-suggestion")
        .header("Content-Type", "application/json")
        .json(&BranchSuggestionRequest { repo: repo.into() })
        .map_err(|_| CallError::Rejected(t!(ServerRequestNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection_own(resp).await);
    }
    resp.json::<BranchSuggestion>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The connection test of GitHub (`POST /api/github/check`): the user and the chosen repos.
pub async fn post_github_check(repos: Vec<String>) -> Result<GitHubCheck, CallError> {
    let request = Request::post("/api/github/check")
        .header("Content-Type", "application/json")
        .json(&RepoCheckRequest { repos })
        .map_err(|_| CallError::Rejected(t!(ServerCheckNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection_own(resp).await);
    }
    resp.json::<GitHubCheck>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}
