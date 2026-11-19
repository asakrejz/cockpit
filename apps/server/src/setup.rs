//! Setup by click: routes for the assistant in the app.
//!
//! * `GET /api/setup`: current, non-secret values and which keys are present
//! * `PUT /api/config`: writes client IDs, Jira, repos as well as agent provider and endpoint to `config.toml`
//!   (atomically, comments and other values stay) and applies them without restart
//! * `PUT /api/secrets/{name}`: stores a key in the .env file; `DELETE` deletes a
//!   token (`jira`, `github`) again ("Trennen")
//! * `POST /api/jira/check`: connection test for Jira via API token (read only)
//! * `GET /api/github/repos`: the repos the GitHub access can reach (read only)
//! * `POST /api/github/branch-suggestion`: a branch pattern proposed from the repo's CONTRIBUTING.md (read only)
//! * `GET /api/github/access`: can the access see every configured repo? (read only)
//! * `POST /api/github/check`: connection test for GitHub: user and chosen repos (read only)
//!
//! All three run behind the check from `access` (loopback, host, origin, content type).
//! A key only occurs in the body of the request, is logged nowhere and never sent back;
//! the answer only says "set".

use std::io::Write;
use std::path::Path;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cockpit_core::api::{
    AgentReposInput, ConfigChange, ConfigResponse, JiraCause, JiraCheck, KeyInput, KeyResponse,
    RepoCheck, RepoCheckRequest, RepoCheckResponse, RepoState, Setup,
};
use cockpit_core::setup::{
    KEY_NAMES, client_id_read, endpoint_url_read, jira_site_read, key_deletable, key_name_allowed,
    key_verify, projects_read, repos_read,
};
use cockpit_i18n::{Msg, msg};
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::Service;
use cockpit_integrations::{Error, Secret};
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::app::{AppState, error_msg};
use crate::config::{Config, ConfigError};
use crate::db;
use crate::oauth::redirect_uri;
use crate::secrets;

mod config_write;
pub use config_write::*;

mod checks;
pub use checks::*;

mod access;
pub use access::*;

mod github;
pub use github::*;

mod messages;
use messages::{github_cause_msg, jira_cause_msg};

impl AppState {
    /// Takes over the addresses and accounts from the configuration into the database (without keys).
    /// Blockierend.
    pub(crate) fn connections_adopt(&self, config: &Config) -> Result<(), Msg> {
        let conn = self
            .inner
            .db
            .lock()
            .map_err(|_| msg!(ServerSetupDbLocked))?;
        if let Some(g) = &config.github {
            db::connection_set(&conn, secrets::GITHUB, &g.api_url, None)
                .map_err(|e| Msg::raw(e.to_string()))?;
        }
        // With OAuth Jira has no address of its own; the sign-in chooses the site.
        if let Some(j) = config.jira.as_ref().filter(|j| !j.site.is_empty()) {
            db::connection_set(&conn, secrets::JIRA, &j.site, Some(&j.email))
                .map_err(|e| Msg::raw(e.to_string()))?;
        }
        Ok(())
    }

    /// Changes `config.toml` and applies the result. Blocking.
    fn config_change(&self, a: &ConfigChange) -> Result<ConfigResponse, (StatusCode, Msg)> {
        let Some(path) = self.inner.settings.config_path.clone() else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                msg!(ServerSetupNoConfigPath),
            ));
        };
        let _lock = self.inner.config_lock.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                msg!(ServerSetupInternalError),
            )
        })?;
        let alt = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                tracing::warn!("config.toml not readable: {e}");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    msg!(
                        ServerSetupConfigUnreadable,
                        path = path.display().to_string()
                    ),
                ));
            }
        };
        let (updated, config) =
            change(&alt, a).map_err(|m| (StatusCode::UNPROCESSABLE_ENTITY, m))?;
        let before = self.sources();
        if updated != alt {
            atomic_write(&path, &updated).map_err(|e| {
                tracing::warn!("config.toml not written: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    msg!(
                        ServerSetupConfigUnwritable,
                        path = path.display().to_string(),
                        kind = e.kind().to_string()
                    ),
                )
            })?;
        }
        self.connections_adopt(&config)
            .map_err(|m| (StatusCode::INTERNAL_SERVER_ERROR, m))?;
        self.config_apply(&config);
        tracing::info!("config.toml updated and applied");

        // Another app means: the old tokens belong to the old app.
        let other_app = |alt: Option<&String>, updated: Option<&String>| {
            alt.is_some() && updated.is_some() && alt != updated
        };
        let id_alt_g = before.github_oauth.as_ref().map(|o| &o.client_id);
        let id_new_g = config.github_oauth.as_ref().map(|o| &o.client_id);
        let id_alt_a = before.atlassian.as_ref().map(|o| &o.client_id);
        let id_new_a = config.atlassian.as_ref().map(|o| &o.client_id);
        let hint = (other_app(id_alt_g, id_new_g) || other_app(id_alt_a, id_new_a))
            .then(|| msg!(ServerSetupClientIdChanged));
        Ok(ConfigResponse {
            stored: true,
            takes_effect_immediately: true,
            hint: hint.as_ref().map(Msg::de),
            msg: hint,
        })
    }

    /// The current, non-secret values. Blocking (.env file).
    fn setup(&self) -> Setup {
        let q = self.sources();
        let choice = self.agent_choice();
        let port = self.inner.port;
        Setup {
            jira_site: q.jira.as_ref().map(|j| j.site.clone()).unwrap_or_default(),
            jira_email: q.jira.as_ref().map(|j| j.email.clone()).unwrap_or_default(),
            jira_projects: q
                .jira
                .as_ref()
                .map(|j| j.projects.clone())
                .unwrap_or_default(),
            atlassian_client_id: q.atlassian.as_ref().map(|a| a.client_id.clone()),
            atlassian_site: q.atlassian.as_ref().and_then(|a| a.site.clone()),
            github_client_id: q.github_oauth.as_ref().map(|o| o.client_id.clone()),
            github_repos: q
                .github
                .as_ref()
                .map(|g| g.repos.clone())
                .unwrap_or_default(),
            // One list for the assistant: repos of `[github] repos` (older) are part of it.
            agent_repos: self.agent_repos_config().unified(
                &q.github
                    .as_ref()
                    .map(|g| g.repos.clone())
                    .unwrap_or_default(),
            ),
            provider: choice.provider,
            endpoint_url: choice.endpoint.as_ref().and_then(|e| e.url.clone()),
            endpoint_model: choice.endpoint.and_then(|e| e.model),
            callback_github: redirect_uri(Service::GitHub, port),
            callback_atlassian: redirect_uri(Service::Atlassian, port),
            webhooks: self.webhooks_info(),
            keys: KEY_NAMES
                .iter()
                .map(|n| KeyResponse {
                    name: (*n).to_string(),
                    set: self.inner.secrets.has(n).unwrap_or(false),
                })
                .collect(),
        }
    }
}

/// `GET /api/setup`.
pub async fn read(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.setup()).await {
        Ok(e) => Json(e).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `PUT /api/config`.
pub async fn config(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(a) = serde_json::from_slice::<ConfigChange>(&body) else {
        return error_msg(StatusCode::BAD_REQUEST, msg!(ServerSetupInputsUnreadable));
    };
    match tokio::task::spawn_blocking(move || state.config_change(&a)).await {
        Ok(Ok(answer)) => Json(answer).into_response(),
        Ok(Err((status, message))) => error_msg(status, message),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `PUT /api/secrets/{name}`: stores the key in the .env file. The value is only in the
/// body; it is neither logged nor sent back, and error messages never name it.
pub async fn key(
    State(state): State<AppState>,
    UrlPath(name): UrlPath<String>,
    body: Bytes,
) -> Response {
    if !key_name_allowed(&name) {
        return error_msg(
            StatusCode::NOT_FOUND,
            msg!(ServerSetupKeyUnknown, name = &name),
        );
    }
    let Ok(input) = serde_json::from_slice::<KeyInput>(&body) else {
        return error_msg(StatusCode::BAD_REQUEST, msg!(ServerSetupInputUnreadable));
    };
    let value = match key_verify(&input.value) {
        Ok(w) => w,
        Err(m) => return error_msg(StatusCode::UNPROCESSABLE_ENTITY, m),
    };
    let secrets = state.inner.secrets.clone();
    let n = name.clone();
    let result = tokio::task::spawn_blocking(move || secrets.set(&n, &Secret::new(value))).await;
    match result {
        Ok(Ok(())) => {
            tracing::info!("Key for {name} stored");
            Json(KeyResponse { name, set: true }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Key for {name} not stored: {e}");
            error_msg(
                StatusCode::INTERNAL_SERVER_ERROR,
                msg!(ServerSetupEnvNotWritable),
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `DELETE /api/secrets/{name}`: deletes a token from the .env file ("Trennen"). Only the tokens
/// `jira` and `github` are allowed; a key that does not exist is no error.
pub async fn key_delete(State(state): State<AppState>, UrlPath(name): UrlPath<String>) -> Response {
    if !key_deletable(&name) {
        return error_msg(
            StatusCode::NOT_FOUND,
            msg!(ServerSetupKeyNotDeletable, name = &name),
        );
    }
    let secrets = state.inner.secrets.clone();
    let n = name.clone();
    match tokio::task::spawn_blocking(move || secrets.delete(&n)).await {
        Ok(Ok(())) => {
            tracing::info!("Key for {name} deleted");
            Json(KeyResponse { name, set: false }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Key for {name} not deleted: {e}");
            error_msg(
                StatusCode::INTERNAL_SERVER_ERROR,
                msg!(ServerSetupEnvNotWritable),
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests;
