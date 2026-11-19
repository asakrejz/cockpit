//! The HTTP part: routes under `/api`, behind them the built web app.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use cockpit_core::api::{ActionRequest, ErrorBody, ServerEvent};
use rusqlite::Connection;
use serde::Serialize;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeDir;

use crate::access;
use crate::config::{AgentChoice, Config, Seed};
use crate::container::Cache;
#[cfg(test)]
use crate::db;
use crate::live::Live;
use crate::secrets::{self, SecretStore};
use crate::sync::{ActionFailure, Sources};
use cockpit_core::ServiceStatus;
use cockpit_core::api::{AgentStatus, ContainerStatus, EndpointStatus};
use cockpit_integrations::oauth::Service;

mod health;
pub use health::*;

mod state;
pub use state::*;

mod handlers;
pub(crate) use handlers::*;

mod cleanup_routes;
use cleanup_routes::*;

/// This often the simulated agent takes a step (as in the demo mode of the app).
pub const AGENT_TICK: Duration = Duration::from_millis(2200);
/// This often a sign of life comes on the event connection.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Builds the router. `dist` is the folder of the built web app.
pub fn router(state: AppState, dist: PathBuf) -> Router {
    let mut ports = vec![state.inner.port];
    ports.extend(state.inner.settings.dev_port);
    let access = access::Access {
        dev: state.inner.settings.dev_port.is_some(),
        ports,
    };
    let protected = Router::new()
        .route("/api/health", get(health))
        .route("/api/workspace", get(workspace))
        .route("/api/actions", post(action))
        .route("/api/sync", post(sync))
        .route("/api/events", get(events))
        .route("/api/jira/projects", get(jira_projects))
        .route("/api/jira/assignable", get(jira_assignable))
        .route(
            "/api/jira/attachment/{id}",
            get(crate::attachments::content),
        )
        .route(
            "/api/jira/attachment/{id}/thumbnail",
            get(crate::attachments::thumbnail),
        )
        .route("/api/agent/models", get(agent_models))
        .route("/api/agent/check", get(endpoint_check))
        .route("/api/agent/pull-request", get(pull_request_preview))
        .route(
            "/api/agent/cleanup",
            get(cleanup_preview).post(cleanup_apply),
        )
        .route("/api/agent/branch", get(branch_check).post(branch_delete))
        .route("/api/review/commits", get(crate::review_panel::commits))
        .route("/api/review/commit", get(crate::review_panel::commit))
        .route("/api/agent/check/container", get(endpoint_check_container))
        .route("/api/jira/check", post(crate::setup::jira_check))
        .route("/api/github/repos/check", post(crate::setup::repos_check))
        .route("/api/github/repos", get(crate::setup::repos_list))
        .route("/api/github/check", post(crate::setup::github_check_route))
        .route("/api/github/access", get(crate::setup::github_access_route))
        .route(
            "/api/github/branch-suggestion",
            post(crate::setup::branch_suggestion_route),
        )
        .route(
            "/api/agent/image",
            get(agent_image_read).post(agent_image_build),
        )
        .route("/api/setup", get(crate::setup::read))
        .route("/api/config", put(crate::setup::config))
        .route(
            "/api/secrets/{name}",
            put(crate::setup::key).delete(crate::setup::key_delete),
        )
        .route("/api/oauth/{service}/start", get(crate::oauth::start))
        .route("/api/oauth/{service}/callback", get(crate::oauth::callback))
        .route("/api/oauth/{service}/revoke", post(crate::oauth::revoke))
        .route(
            "/api/setup/webhook-secret/{source}",
            post(crate::webhooks::secret_new),
        )
        .route(
            "/api/setup/webhook-test/{source}",
            post(crate::webhooks::test),
        )
        .route("/api/{*rest}", any(api_unknown))
        // Applies only to `/api`, not to the files of the app. Added routes below are outside.
        .layer(axum::middleware::from_fn_with_state(
            access,
            access::protect,
        ));
    // The two webhook routes are the only ones that accept a foreign `Host` (a tunnel forwards
    // its public name). They do their own checks (switch, secret, signature); see `webhooks`.
    let webhooks = Router::new()
        .route(crate::webhooks::GITHUB_PATH, any(crate::webhooks::github))
        .route(crate::webhooks::JIRA_PATH, any(crate::webhooks::jira));
    protected
        .merge(webhooks)
        .with_state(state)
        .fallback_service(ServeDir::new(dist))
}

#[cfg(test)]
mod tests;
