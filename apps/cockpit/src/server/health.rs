//! The state of the sign-in, of the agent endpoint and of the container runtime
//! (`GET /api/health`).

use cockpit_core::ServiceStatus;
use cockpit_core::api::{AgentStatus, ContainerStatus, EndpointStatus};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection};

/// How `GET /api/health` reports the sign-in per service.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Services {
    #[serde(default)]
    pub github: ServiceStatus,
    #[serde(default)]
    pub jira: ServiceStatus,
}

/// The part of `GET /api/health` that the page "Setup" needs. Never a key:
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
    /// The folder `config/` with configuration and database: path and warnings.
    #[serde(default)]
    pub config_files: Option<ConfigFilesStatus>,
    /// Saved workspaces that could not be read and are kept aside in the database.
    #[serde(default)]
    pub unreadable_state: Option<UnreadableState>,
}

/// Saved workspaces the server could not read; it kept them unchanged and started over.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct UnreadableState {
    #[serde(default)]
    pub count: u32,
    /// The notice as code and parameters (shown in the current language).
    #[serde(default)]
    pub notice: cockpit_i18n::Msg,
}

/// Path of the folder `config/` and its warnings (Git, permissions).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ConfigFilesStatus {
    #[serde(default)]
    pub path: String,
    /// The warnings as code and parameters (shown in the current language).
    #[serde(default)]
    pub warnings: Vec<cockpit_i18n::Msg>,
}

/// Path of the `.env` file and warnings of the server (tracked by Git, not ignored, permissions).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct EnvFileStatus {
    #[serde(default)]
    pub path: String,
    /// The warnings as code and parameters (shown in the current language).
    #[serde(default)]
    pub warnings: Vec<cockpit_i18n::Msg>,
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
