//! Sign-in via OAuth in the backend: redirect, callback, refresh, revoke.
//!
//! The rules (PKCE, `state`, exchange, site choice) are in `cockpit_integrations::oauth`;
//! here the course runs with .env file and database. Client secret and tokens only lie in the
//! .env file, never in the database, never in the log, never in the browser. The browser only sees
//! redirects and messages.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use cockpit_core::api::{ErrorBody, RevokeResponse};
use cockpit_core::{AuthKind, ServiceStatus};
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::{
    self, App, Service, SignIns, Site, Tokens, jira_base, site_choose,
};
use cockpit_integrations::{Error, Secret};

use crate::app::{AppState, now_ms};
use crate::db;
use crate::secrets;

mod flow;
mod refresh;
mod tokens;

const SITE_ID: &str = "atlassian.cloud_id";
const SITE_NAME: &str = "atlassian.site_name";

fn secret_name(d: Service) -> &'static str {
    match d {
        Service::GitHub => secrets::GITHUB_OAUTH,
        Service::Atlassian => secrets::ATLASSIAN_OAUTH,
    }
}

fn tokens_name(d: Service) -> &'static str {
    match d {
        Service::GitHub => secrets::GITHUB_OAUTH_TOKENS,
        Service::Atlassian => secrets::ATLASSIAN_OAUTH_TOKENS,
    }
}

/// The step of the assistant in which this service is set up (messages link to it).
pub(crate) fn step_name(d: Service) -> &'static str {
    match d {
        Service::GitHub => "GitHub",
        Service::Atlassian => "Jira",
    }
}

/// "Das Client-Secret fehlt …" – names the field in the assistant, never a terminal command.
pub(crate) fn secret_missing(d: Service) -> String {
    format!(
        "Das Client-Secret fehlt – trage es im Schritt „{}“ im Feld „Client-Secret“ ein.",
        step_name(d)
    )
}

/// The callback address as it is entered in the app of the service. GitHub: `127.0.0.1`,
/// Atlassian: `localhost`.
pub fn redirect_uri(d: Service, port: u16) -> String {
    let host = match d {
        Service::GitHub => "127.0.0.1",
        Service::Atlassian => "localhost",
    };
    format!("http://{host}:{port}/api/oauth/{}/callback", d.id())
}

fn now_s() -> u64 {
    now_ms() / 1000
}

/// A message for the interface; never contains a key.
fn service_error(d: Service, f: &Error) -> String {
    match f {
        Error::NotSignedIn => format!(
            "{} lehnt die Zugangsdaten ab – prüfe Client-ID und Client-Secret im Schritt „{}“.",
            d.name(),
            step_name(d)
        ),
        Error::Network(_) => format!("{} ist nicht erreichbar", d.name()),
        other => format!("{}: {other}", d.name()),
    }
}

/// Percent encoding for parts of an address.
fn encoded(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ───────────── Routes ─────────────

fn parse_service(id: &str) -> Result<Service, ()> {
    Service::from_id(id).ok_or(())
}

fn unknown_service() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: "Unbekannter Dienst".into(),
        }),
    )
        .into_response()
}

/// Back into the app – via the address `127.0.0.1` so that the stored mode of the browser
/// applies, even if the service redirected to `localhost`.
fn back(port: u16, d: Service, result: &Result<String, String>) -> Redirect {
    let base = format!("http://127.0.0.1:{port}/");
    let target = match result {
        Ok(m) => format!(
            "{base}?oauth=ok&service={}&message={}#/setup",
            d.id(),
            encoded(m)
        ),
        Err(m) => format!(
            "{base}?oauth=error&service={}&message={}#/setup",
            d.id(),
            encoded(m)
        ),
    };
    Redirect::to(&target)
}

/// `GET /api/oauth/{service}/start`: redirects to the sign-in of the service.
pub async fn start(State(state): State<AppState>, Path(service): Path<String>) -> Response {
    let Ok(d) = parse_service(&service) else {
        return unknown_service();
    };
    match state.oauth_start(d).await {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(m) => back(state.inner.port, d, &Err(m)).into_response(),
    }
}

/// `GET /api/oauth/{service}/callback`: receives the browser after the sign-in.
pub async fn callback(
    State(state): State<AppState>,
    Path(service): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Ok(d) = parse_service(&service) else {
        return unknown_service();
    };
    let result = state.oauth_callback(d, &q).await;
    if let Err(m) = &result {
        tracing::warn!("{}: sign-in failed: {m}", d.id());
    }
    back(state.inner.port, d, &result).into_response()
}

/// `POST /api/oauth/{service}/revoke`: disconnects the service.
pub async fn revoke(State(state): State<AppState>, Path(service): Path<String>) -> Response {
    let Ok(d) = parse_service(&service) else {
        return unknown_service();
    };
    match state.oauth_revoke(d).await {
        Ok(a) => Json(a).into_response(),
        Err(m) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody { error: m }),
        )
            .into_response(),
    }
}

/// State for `AppState`: open sign-ins.
pub fn new_sign_ins() -> std::sync::Mutex<SignIns> {
    std::sync::Mutex::new(SignIns::default())
}

#[cfg(test)]
mod tests;
