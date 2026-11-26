//! Optional webhooks: GitHub and Jira tell the server that something changed, so it syncs
//! right away instead of waiting for the next poll.
//!
//! This is the only part of the server that is meant to be reachable from the internet (through
//! a tunnel the user runs). Everything else stays loopback-only (`access`). The rules:
//!
//! * A webhook is only a **trigger**. The payload is never used as data; after a valid one the
//!   server syncs the affected source over its authenticated API (debounced).
//! * Off by default (`[server] webhooks = false`: both routes answer 404). Without a secret in
//!   the `.env` file (`GITHUB_WEBHOOK_SECRET`, `JIRA_WEBHOOK_SECRET`) nothing is accepted.
//! * HMAC-SHA256 over the raw body, compared in constant time. Jira may alternatively use the
//!   secret as `?token=` for senders that cannot sign (weaker).
//! * Only `application/json`, at most [`MAX_BODY`] bytes, a rate limit per source, duplicate
//!   deliveries are ignored for a while.
//! * Answers say nothing about *why* a request failed. Logs contain at most the event type.
//!
//! The two routes also work with a foreign `Host` header (a tunnel forwards the public name).
//! That exception is only these two routes: they sit outside the `access` layer, every other
//! route is still behind it.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use axum::Json;
use axum::extract::{ConnectInfo, Path as UrlPath, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use cockpit_core::Source;
use cockpit_core::api::{
    WebhookSecretResponse, WebhookSourceInfo, WebhookTestResponse, WebhooksInfo,
};
use cockpit_integrations::Secret;
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::app::{AppState, error_msg, now_ms};
use crate::secrets;
use cockpit_i18n::{Msg, msg};

mod management;
pub use management::*;

mod signature;
pub use signature::*;

mod hub;
pub use hub::*;

/// Largest accepted body. GitHub caps its payloads at 25 MB, but we never read them.
pub const MAX_BODY: usize = 1024 * 1024;
/// At most one sync per source in this time; more webhooks in between merge into one.
pub const DEBOUNCE: Duration = Duration::from_secs(5);
/// Requests per source in [`RATE_WINDOW`] (valid or not); more get `429`.
pub const RATE_LIMIT: usize = 120;
const RATE_WINDOW: Duration = Duration::from_secs(60);
/// A delivery seen again within this time is ignored.
const REPLAY_TTL: Duration = Duration::from_secs(600);
const REPLAY_MAX: usize = 512;
/// Webhooks count as "arriving regularly" if the last one is younger than this many intervals.
pub const HEALTHY_INTERVALS: u32 = 2;

pub const GITHUB_PATH: &str = "/api/webhooks/github";
pub const JIRA_PATH: &str = "/api/webhooks/jira";

/// GitHub events that can change what the sync shows.
const GITHUB_EVENTS: &[&str] = &[
    "pull_request",
    "pull_request_review",
    "pull_request_review_comment",
    "check_run",
    "check_suite",
    "issue_comment",
];
/// Jira events that can change what the sync shows.
const JIRA_EVENTS: &[&str] = &[
    "jira:issue_created",
    "jira:issue_updated",
    "jira:issue_deleted",
    "comment_created",
    "comment_updated",
    "comment_deleted",
];
/// Event type the local "Test" button sends (not a real delivery).
const TEST_EVENT: &str = "test (lokal)";

/// The settings that can change while the server runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSettings {
    pub enabled: bool,
    pub public_url: Option<String>,
    /// Poll interval of a source while its webhooks arrive regularly.
    pub fallback_s: u64,
    /// Minimum time between two syncs triggered by webhooks (tests shorten it).
    pub debounce: Duration,
}

impl Default for WebhookSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            public_url: None,
            fallback_s: crate::config::fallback_s(crate::config::DEFAULT_INTERVAL_S, None),
            debounce: DEBOUNCE,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Signature
// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// State: debounce, rate limit, replay, last received
// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// Receiving
// ---------------------------------------------------------------------------------------------

fn status(code: StatusCode) -> Response {
    code.into_response()
}

/// Letters, digits and `_ : . -`, at most 64 – what an event type may look like.
fn event_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-'))
}

fn header_text<'a>(head: &'a HeaderMap, name: &str) -> Option<&'a str> {
    head.get(name).and_then(|v| v.to_str().ok())
}

fn is_json(head: &HeaderMap) -> bool {
    header_text(head, CONTENT_TYPE.as_str())
        .and_then(|c| c.split(';').next())
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
}

/// The `token` of a query string (`?token=...`), if any.
fn query_token(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .find_map(|p| p.strip_prefix("token="))
        .filter(|t| !t.is_empty())
}

/// `POST /api/webhooks/github`.
pub async fn github(State(state): State<AppState>, req: Request) -> Response {
    receive(state, Source::GitHub, req).await
}

/// `POST /api/webhooks/jira`.
pub async fn jira(State(state): State<AppState>, req: Request) -> Response {
    receive(state, Source::Jira, req).await
}

fn name(source: Source) -> &'static str {
    match source {
        Source::GitHub | Source::GitHubProject => "GitHub",
        Source::Jira => "Jira",
    }
}

fn secret_name(source: Source) -> &'static str {
    match source {
        Source::GitHub | Source::GitHubProject => secrets::GITHUB_WEBHOOK,
        Source::Jira => secrets::JIRA_WEBHOOK,
    }
}

async fn receive(state: AppState, source: Source, req: Request) -> Response {
    let hub = &state.inner.webhooks;
    // Off (or not yet set up): the route does not exist.
    if !hub.settings().enabled {
        return status(StatusCode::NOT_FOUND);
    }
    if req.method() != Method::POST {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    // The server listens on 127.0.0.1 only, so a tunnel client arrives from there.
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    if !peer.is_some_and(|ip| ip.is_loopback()) {
        return status(StatusCode::FORBIDDEN);
    }
    if !is_json(req.headers()) {
        return status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let now = Instant::now();
    if !hub.rate_ok(source, now) {
        let mut answer = status(StatusCode::TOO_MANY_REQUESTS);
        answer
            .headers_mut()
            .insert("retry-after", axum::http::HeaderValue::from_static("60"));
        return answer;
    }
    // Without a secret nothing is accepted. Same answer as "off" so the outside learns nothing.
    let s = state.clone();
    let secret: Option<Secret> = tokio::task::spawn_blocking(move || s.token(secret_name(source)))
        .await
        .ok()
        .flatten();
    let Some(secret) = secret else {
        tracing::warn!(
            "Webhook for {}: no secret in the .env file, ignored",
            name(source)
        );
        return status(StatusCode::NOT_FOUND);
    };
    let declared =
        header_text(req.headers(), CONTENT_LENGTH.as_str()).and_then(|l| l.parse::<usize>().ok());
    if declared.is_some_and(|l| l > MAX_BODY) {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let (parts, body) = req.into_parts();
    let Ok(body) = axum::body::to_bytes(body, MAX_BODY).await else {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    };

    // Authentication. GitHub: X-Hub-Signature-256. Jira: X-Hub-Signature, otherwise (senders
    // that cannot sign) the secret as ?token=.
    let signed = match source {
        Source::GitHub | Source::GitHubProject => {
            header_text(&parts.headers, "x-hub-signature-256")
        }
        Source::Jira => header_text(&parts.headers, "x-hub-signature"),
    };
    let authentic = match (signed, source) {
        (Some(_), _) | (None, Source::GitHub | Source::GitHubProject) => {
            signature_valid(secret.expose().as_bytes(), &body, signed)
        }
        (None, Source::Jira) => query_token(parts.uri.query())
            .is_some_and(|t| equal_constant_time(t.as_bytes(), secret.expose().as_bytes())),
    };
    if !authentic {
        tracing::warn!("Webhook for {}: authentication failed", name(source));
        return status(StatusCode::UNAUTHORIZED);
    }

    // The event type is all we take from the request.
    let test = parts.headers.contains_key("x-cockpit-test");
    let event = match source {
        Source::GitHub | Source::GitHubProject => header_text(&parts.headers, "x-github-event")
            .filter(|e| event_name_ok(e))
            .map(str::to_string),
        Source::Jira => serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("webhookEvent")
                    .and_then(|e| e.as_str())
                    .map(str::to_string)
            })
            .filter(|e| event_name_ok(e))
            .or_else(|| Some("unknown".into())),
    };
    let Some(event) = event else {
        return status(StatusCode::BAD_REQUEST);
    };

    // Replay: the same delivery ID, or the same body again, within ten minutes.
    let mut keys = vec![format!("body:{}", hex(&Sha256::digest(&body)))];
    let delivery = match source {
        Source::GitHub | Source::GitHubProject => header_text(&parts.headers, "x-github-delivery"),
        Source::Jira => header_text(&parts.headers, "x-atlassian-webhook-identifier"),
    };
    if let Some(d) = delivery.filter(|d| d.len() <= 100 && !d.is_empty()) {
        keys.push(format!("id:{d}"));
    }
    if !test && hub.seen_before(source, now, &keys) {
        tracing::info!("Webhook for {}: duplicate delivery ignored", name(source));
        return status(StatusCode::ACCEPTED);
    }

    let is_ping = test || event == "ping" || event == "cockpit:test";
    let relevant = match source {
        Source::GitHub | Source::GitHubProject => GITHUB_EVENTS.contains(&event.as_str()),
        Source::Jira => JIRA_EVENTS.contains(&event.as_str()),
    };
    let shown = if test { TEST_EVENT } else { &event };
    hub.received(
        source,
        now_ms(),
        shown,
        (relevant && !is_ping).then_some(now),
    );
    tracing::info!("Webhook for {}: event {shown}", name(source));
    if is_ping {
        return Json(serde_json::json!({ "ok": true })).into_response();
    }
    if !relevant {
        return status(StatusCode::NO_CONTENT);
    }
    state.webhook_trigger(source);
    status(StatusCode::ACCEPTED)
}

// ---------------------------------------------------------------------------------------------
// Assistant: generate a secret, test locally
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests;
