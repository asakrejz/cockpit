//! Server mode from the browser's point of view: requests to the local backend
//! (`cockpit-server`) and the event stream. The shapes of the data are in
//! `cockpit_core::api`. No key is here and none is stored.

use cockpit_core::ServiceStatus;
use cockpit_core::api::{
    ActionRequest, ActionResponse, AgentStatus, BranchSuggestion, BranchSuggestionRequest,
    ConfigChange, ConfigResponse, ContainerCheck, ContainerStatus, EndpointCheck, EndpointStatus,
    ErrorBody, GitHubCheck, GitHubRepoList, ImageBuild, JiraCheck, JiraProjectsResponse, KeyInput,
    KeyResponse, ModelList, RepoCheckRequest, RevokeResponse, ServerEvent, Setup, SyncResponse,
    WebhookSecretResponse, WebhookTestResponse,
};
use cockpit_core::{Action, Workspace, persist};
use feature_ui::t;
use gloo_net::http::Request;
use web_sys::wasm_bindgen::{JsCast, closure::Closure};

mod cleanup;
pub use cleanup::*;

mod panel;
pub use panel::*;

/// Why a request to the server gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The server saw the request and rejected it – with a message for the interface.
    Rejected(String),
    /// No answer: the server is not running or is not at this address.
    Unreachable,
}

/// A text of the server in the current language: the coded message if there is one, otherwise
/// the plain text (which comes from outside or from an older server).
pub fn shown(text: &str, msg: Option<&cockpit_i18n::Msg>) -> String {
    msg.map_or_else(
        || text.to_string(),
        |m| m.render(feature_ui::lang_untracked()),
    )
}

impl CallError {
    pub fn message(&self) -> String {
        match self {
            CallError::Rejected(m) => m.clone(),
            CallError::Unreachable => cockpit_core::notice::server_unreachable_title()
                .render(feature_ui::lang_untracked()),
        }
    }
}

/// The message from an error response, otherwise a general one.
async fn rejection(resp: gloo_net::http::Response) -> CallError {
    let status = resp.status();
    let server_message = resp
        .json::<ErrorBody>()
        .await
        .map(|e| e.in_lang(feature_ui::lang_untracked()))
        .ok();
    CallError::Rejected(
        cockpit_core::notice::http_failure_msg(status, server_message.as_deref())
            .render(feature_ui::lang_untracked()),
    )
}

/// Like [`rejection`], but the server's own sentence wins (the GitHub calls of the assistant
/// already say in plain text what is wrong, e.g. which token problem).
async fn rejection_own(resp: gloo_net::http::Response) -> CallError {
    match resp.json::<ErrorBody>().await {
        Ok(e) if !e.error.trim().is_empty() => {
            CallError::Rejected(e.in_lang(feature_ui::lang_untracked()))
        }
        _ => CallError::Rejected(t!(ServerGithubUnavailable)),
    }
}

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

/// "Jetzt abgleichen": triggers the sync with Jira and GitHub (`POST /api/sync`).
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

/// Preview of the pull request for the branch of a finished real run
/// (`GET /api/agent/pull-request?run=…`). Creates nothing.
pub async fn fetch_pull_request_preview(
    run: &str,
) -> Result<cockpit_core::api::PullRequestPreview, CallError> {
    let resp = Request::get("/api/agent/pull-request")
        .query([("run", run)])
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

/// Open event stream. On closing (`Drop`) the connection is ended.
pub struct Events {
    source: web_sys::EventSource,
}

impl Drop for Events {
    fn drop(&mut self) {
        self.source.close();
    }
}

impl Events {
    /// Opens `GET /api/events`. After an interruption the browser reconnects itself; then
    /// `on_open` comes again.
    pub fn open(
        on_open: impl Fn() + 'static,
        on_event: impl Fn(ServerEvent) + 'static,
        on_error: impl Fn() + 'static,
    ) -> Option<Self> {
        let source = web_sys::EventSource::new("/api/events").ok()?;
        let open = Closure::<dyn Fn()>::new(on_open);
        let error = Closure::<dyn Fn()>::new(on_error);
        let message =
            Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                let parsed = ev
                    .data()
                    .as_string()
                    .and_then(|text| serde_json::from_str::<ServerEvent>(&text).ok());
                if let Some(event) = parsed {
                    on_event(event);
                }
            });
        source.set_onopen(Some(open.as_ref().unchecked_ref()));
        source.set_onerror(Some(error.as_ref().unchecked_ref()));
        source.set_onmessage(Some(message.as_ref().unchecked_ref()));
        // The callbacks live as long as the page is open.
        open.forget();
        error.forget();
        message.forget();
        Some(Self { source })
    }
}

/// How `GET /api/health` reports the sign-in per service.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Services {
    #[serde(default)]
    pub github: ServiceStatus,
    #[serde(default)]
    pub jira: ServiceStatus,
}

/// The part of `GET /api/health` that the page "Einrichten" needs. Never a key:
/// the server only reports yes/no.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Health {
    #[serde(default)]
    pub sign_in: Services,
    #[serde(default)]
    pub endpoint: EndpointStatus,
    #[serde(default)]
    pub container: ContainerStatus,
    #[serde(default)]
    pub agent: AgentStatus,
    /// Where the `.env` file with the keys lives and what to warn about (never a value).
    #[serde(default)]
    pub env_file: Option<EnvFileStatus>,
    /// The folder `config/` with configuration and database: path, warnings, old files.
    #[serde(default)]
    pub config_files: Option<ConfigFilesStatus>,
}

/// Path of the folder `config/`, warnings (Git, permissions) and hints about old files.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ConfigFilesStatus {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The warnings as code and parameters (shown in the current language).
    #[serde(default)]
    pub warning_msgs: Vec<cockpit_i18n::Msg>,
    #[serde(default)]
    pub notices: Vec<String>,
    #[serde(default)]
    pub notice_msgs: Vec<cockpit_i18n::Msg>,
}

/// Path of the `.env` file and warnings of the server (tracked by Git, not ignored, permissions).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct EnvFileStatus {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The warnings as code and parameters (shown in the current language).
    #[serde(default)]
    pub warning_msgs: Vec<cockpit_i18n::Msg>,
}

/// The texts of a list: the coded messages in the current language, or the plain texts.
pub fn shown_list(texts: &[String], msgs: &[cockpit_i18n::Msg]) -> Vec<String> {
    if msgs.is_empty() {
        texts.to_vec()
    } else {
        msgs.iter()
            .map(|m| m.render(feature_ui::lang_untracked()))
            .collect()
    }
}

/// State of the sign-in, of the agent endpoint and of the container runtime (`GET /api/health`).
pub async fn fetch_health() -> Result<Health, CallError> {
    let resp = Request::get("/api/health")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<Health>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Kind and state of the sign-in at GitHub and Jira (`GET /api/health`).
pub async fn fetch_services() -> Result<Services, CallError> {
    fetch_health().await.map(|h| h.sign_in)
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

/// State of "Image bauen" (`GET /api/agent/image`).
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

/// Starts "Image bauen" (`POST /api/agent/image`); the build continues in the server.
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

/// "Trennen": revokes the sign-in (`POST /api/oauth/{service}/revoke`). `service`:
/// `github` or `atlassian`.
pub async fn post_revoke(service: &str) -> Result<RevokeResponse, CallError> {
    let request = Request::post(&format!("/api/oauth/{service}/revoke"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerDisconnectNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<RevokeResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The current, non-secret values for the assistant (`GET /api/setup`).
pub async fn fetch_setup() -> Result<Setup, CallError> {
    let resp = Request::get("/api/setup")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<Setup>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Writes client IDs, Jira, repos or provider and endpoint to `config.toml` (`PUT /api/config`).
pub async fn put_config(change: &ConfigChange) -> Result<ConfigResponse, CallError> {
    let request = Request::put("/api/config")
        .header("Content-Type", "application/json")
        .json(change)
        .map_err(|_| CallError::Rejected(t!(ServerInputsNotSent)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ConfigResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Stores a key in the .env file of the server (`PUT /api/secrets/{name}`). The value
/// only goes into the body of this request – not into the address, not into the memory of the browser.
/// The answer only says "set".
pub async fn put_key(name: &str, value: String) -> Result<KeyResponse, CallError> {
    let request = Request::put(&format!("/api/secrets/{name}"))
        .header("Content-Type", "application/json")
        .json(&KeyInput { value })
        .map_err(|_| CallError::Rejected(t!(ServerInputNotSent)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<KeyResponse>()
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

/// Deletes a token from the .env file (`DELETE /api/secrets/{name}`), "Trennen".
pub async fn delete_key(name: &str) -> Result<KeyResponse, CallError> {
    let request = Request::delete(&format!("/api/secrets/{name}"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerDisconnectNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<KeyResponse>()
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

/// Generates a new webhook secret on the server (`POST /api/setup/webhook-secret/{source}`).
/// The answer carries the value once, so it can be pasted into GitHub or Jira.
pub async fn post_webhook_secret(source: &str) -> Result<WebhookSecretResponse, CallError> {
    let request = Request::post(&format!("/api/setup/webhook-secret/{source}"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerRequestNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<WebhookSecretResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Sends a signed test request from the server to itself (`POST /api/setup/webhook-test/{source}`).
pub async fn post_webhook_test(source: &str) -> Result<WebhookTestResponse, CallError> {
    let request = Request::post(&format!("/api/setup/webhook-test/{source}"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerTestNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<WebhookTestResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}
