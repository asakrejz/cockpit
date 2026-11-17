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
//!   secret as `?token=` for senders that cannot sign (weaker, see `docs/security.md`).
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

use crate::app::{AppState, error, now_ms};
use crate::secrets;

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

/// HMAC-SHA256 (RFC 2104) built from SHA-256.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// Compares in constant time (for equal lengths; the length itself is not secret).
pub fn equal_constant_time(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// The header looks like `sha256=<hex>` (hex in any case). Valid if it is the HMAC of `body`.
pub fn signature_valid(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(given) = header
        .and_then(|h| h.trim().strip_prefix("sha256="))
        .and_then(unhex)
    else {
        return false;
    };
    equal_constant_time(&hmac_sha256(secret, body), &given)
}

/// `sha256=<hex>` of the body – what the sender puts in the header.
pub fn signature_header(secret: &[u8], body: &[u8]) -> String {
    format!("sha256={}", hex(&hmac_sha256(secret, body)))
}

/// A fresh random secret: 32 bytes from the system as 64 hex characters.
pub fn secret_generate() -> Result<String, String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|_| "Kein Zufall verfügbar".to_string())?;
    Ok(hex(&buf))
}

// ---------------------------------------------------------------------------------------------
// State: debounce, rate limit, replay, last received
// ---------------------------------------------------------------------------------------------

/// What to do with a trigger.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    /// A sync is already scheduled; this webhook is merged into it.
    Merged,
    /// Sync after this long (zero = now).
    After(Duration),
}

/// Merges triggers: one sync at once (leading edge), later ones within `debounce` into a single
/// one after it (trailing edge).
#[derive(Debug, Default)]
pub struct Gate {
    last_run: Option<Instant>,
    scheduled: bool,
}

impl Gate {
    pub fn admit(&mut self, now: Instant, debounce: Duration) -> Admit {
        if self.scheduled {
            return Admit::Merged;
        }
        self.scheduled = true;
        let wait = self.last_run.map_or(Duration::ZERO, |l| {
            (l + debounce).saturating_duration_since(now)
        });
        Admit::After(wait)
    }

    pub fn started(&mut self, now: Instant) {
        self.last_run = Some(now);
        self.scheduled = false;
    }
}

#[derive(Default)]
struct PerSource {
    /// When a valid request last arrived (wall clock, for display) and its event type.
    last_received: Option<(u64, String)>,
    /// Last valid delivery of a relevant event (not ping/test): the webhooks "work".
    last_active: Option<Instant>,
    /// When the sync last asked this source.
    last_fetch: Option<Instant>,
    gate: Gate,
    rate: VecDeque<Instant>,
    seen: VecDeque<(Instant, String)>,
}

impl PerSource {
    /// Counts a request; `false` if there were too many in the window.
    fn rate_ok(&mut self, now: Instant) -> bool {
        while self
            .rate
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > RATE_WINDOW)
        {
            self.rate.pop_front();
        }
        if self.rate.len() >= RATE_LIMIT {
            return false;
        }
        self.rate.push_back(now);
        true
    }

    /// `true` if one of the keys (delivery ID, body hash) was seen recently; remembers them.
    fn seen_before(&mut self, now: Instant, keys: &[String]) -> bool {
        while self
            .seen
            .front()
            .is_some_and(|(t, _)| now.saturating_duration_since(*t) > REPLAY_TTL)
        {
            self.seen.pop_front();
        }
        if keys.iter().any(|k| self.seen.iter().any(|(_, s)| s == k)) {
            return true;
        }
        for k in keys {
            if self.seen.len() >= REPLAY_MAX {
                self.seen.pop_front();
            }
            self.seen.push_back((now, k.clone()));
        }
        false
    }
}

/// Everything the webhooks need to remember, in memory only.
pub struct Hub {
    settings: RwLock<WebhookSettings>,
    sources: Mutex<[PerSource; 2]>,
}

fn idx(source: Source) -> usize {
    match source {
        Source::GitHub => 0,
        Source::Jira => 1,
    }
}

impl Hub {
    pub fn new(settings: WebhookSettings) -> Self {
        Self {
            settings: RwLock::new(settings),
            sources: Mutex::new(Default::default()),
        }
    }

    pub fn settings(&self) -> WebhookSettings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Takes over `[server]` after a change in the assistant.
    pub fn apply(&self, enabled: bool, public_url: Option<String>) {
        let mut s = self.settings.write().unwrap_or_else(|e| e.into_inner());
        s.enabled = enabled;
        s.public_url = public_url;
    }

    fn with<R>(&self, source: Source, f: impl FnOnce(&mut PerSource) -> R) -> R {
        let mut all = self.sources.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut all[idx(source)])
    }

    pub fn rate_ok(&self, source: Source, now: Instant) -> bool {
        self.with(source, |p| p.rate_ok(now))
    }

    pub fn seen_before(&self, source: Source, now: Instant, keys: &[String]) -> bool {
        self.with(source, |p| p.seen_before(now, keys))
    }

    pub fn received(&self, source: Source, at_ms: u64, event: &str, active: Option<Instant>) {
        self.with(source, |p| {
            p.last_received = Some((at_ms, event.to_string()));
            if active.is_some() {
                p.last_active = active;
            }
        });
    }

    pub fn last_received(&self, source: Source) -> Option<(u64, String)> {
        self.with(source, |p| p.last_received.clone())
    }

    pub fn admit(&self, source: Source, now: Instant) -> Admit {
        let debounce = self.settings().debounce;
        self.with(source, |p| p.gate.admit(now, debounce))
    }

    pub fn started(&self, source: Source, now: Instant) {
        self.with(source, |p| p.gate.started(now));
    }

    pub fn fetched(&self, source: Source, now: Instant) {
        self.with(source, |p| p.last_fetch = Some(now));
    }

    /// Whether the sync tick should ask `source` now.
    pub fn poll_due(&self, source: Source, now: Instant, base: Duration) -> bool {
        let s = self.settings();
        let (active, fetch) = self.with(source, |p| (p.last_active, p.last_fetch));
        poll_due(
            s.enabled,
            active.map(|t| now.saturating_duration_since(t)),
            fetch.map(|t| now.saturating_duration_since(t)),
            base,
            Duration::from_secs(s.fallback_s),
        )
    }
}

/// The poll decision, with elapsed times instead of clocks (so tests need no waiting).
///
/// Without webhooks, or when the last relevant webhook is older than [`HEALTHY_INTERVALS`]
/// intervals (or none arrived yet), every tick polls. While they arrive regularly, a source is
/// polled only as a safety net every `fallback`; `base / 2` of slack keeps tick jitter from
/// skipping a whole round.
pub fn poll_due(
    enabled: bool,
    since_active: Option<Duration>,
    since_fetch: Option<Duration>,
    base: Duration,
    fallback: Duration,
) -> bool {
    let healthy = enabled && since_active.is_some_and(|a| a <= base * HEALTHY_INTERVALS);
    if !healthy {
        return true;
    }
    since_fetch.is_none_or(|f| f + base / 2 >= fallback)
}

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
        Source::GitHub => "GitHub",
        Source::Jira => "Jira",
    }
}

fn secret_name(source: Source) -> &'static str {
    match source {
        Source::GitHub => secrets::GITHUB_WEBHOOK,
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
        Source::GitHub => header_text(&parts.headers, "x-hub-signature-256"),
        Source::Jira => header_text(&parts.headers, "x-hub-signature"),
    };
    let authentic = match (signed, source) {
        (Some(_), _) | (None, Source::GitHub) => {
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
        Source::GitHub => header_text(&parts.headers, "x-github-event")
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
        Source::GitHub => header_text(&parts.headers, "x-github-delivery"),
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
        Source::GitHub => GITHUB_EVENTS.contains(&event.as_str()),
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

impl AppState {
    /// A valid webhook arrived: sync `source` soon, merged with others (see [`Gate`]).
    pub(crate) fn webhook_trigger(&self, source: Source) {
        let hub = &self.inner.webhooks;
        let Admit::After(wait) = hub.admit(source, Instant::now()) else {
            return;
        };
        let state = self.clone();
        tokio::spawn(async move {
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            state.inner.webhooks.started(source, Instant::now());
            if let Err(e) = state.sync_sources(&[source]).await {
                tracing::warn!("Sync after webhook: {e:?}");
            }
        });
    }

    /// The sources whose turn it is at this tick of the sync.
    pub(crate) fn sources_due(&self) -> Vec<Source> {
        let base = self.sources().interval;
        let now = Instant::now();
        [Source::GitHub, Source::Jira]
            .into_iter()
            .filter(|s| self.inner.webhooks.poll_due(*s, now, base))
            .collect()
    }

    /// The state of the webhooks for the assistant. Blocking (.env file).
    pub(crate) fn webhooks_info(&self) -> WebhooksInfo {
        let hub = &self.inner.webhooks;
        let s = hub.settings();
        let info = |source: Source, path: &str| {
            let last = hub.last_received(source);
            WebhookSourceInfo {
                path: path.to_string(),
                secret_set: self.token(secret_name(source)).is_some(),
                last_received_ms: last.as_ref().map(|l| l.0),
                last_event: last.map(|l| l.1),
            }
        };
        WebhooksInfo {
            enabled: s.enabled,
            public_url: s.public_url,
            fallback_interval_s: s.fallback_s,
            github: info(Source::GitHub, GITHUB_PATH),
            jira: info(Source::Jira, JIRA_PATH),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Assistant: generate a secret, test locally
// ---------------------------------------------------------------------------------------------

fn source_from(text: &str) -> Option<Source> {
    match text {
        "github" => Some(Source::GitHub),
        "jira" => Some(Source::Jira),
        _ => None,
    }
}

/// `POST /api/setup/webhook-secret/{source}`: generates a new random secret, stores it in the
/// `.env` file (replacing an old one) and returns it **this once** so it can be pasted into
/// GitHub or Jira. It cannot be read again.
pub async fn secret_new(
    State(state): State<AppState>,
    UrlPath(source): UrlPath<String>,
) -> Response {
    let Some(source) = source_from(&source) else {
        return error(StatusCode::NOT_FOUND, "Unbekannte Quelle");
    };
    let secret = match secret_generate() {
        Ok(s) => s,
        Err(m) => return error(StatusCode::INTERNAL_SERVER_ERROR, m),
    };
    let store = state.inner.secrets.clone();
    let value = Secret::new(secret.clone());
    match tokio::task::spawn_blocking(move || store.set(secret_name(source), &value)).await {
        Ok(Ok(())) => {
            tracing::info!("Webhook secret for {} generated", name(source));
            Json(WebhookSecretResponse { secret }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Webhook secret for {} not stored: {e}", name(source));
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Die .env-Datei im Projektordner ließ sich nicht beschreiben. Prüfe die Rechte von Ordner und Datei.",
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/setup/webhook-test/{source}`: sends a signed test request to this server over
/// loopback and reports what came back. Calls no external service.
pub async fn test(State(state): State<AppState>, UrlPath(source): UrlPath<String>) -> Response {
    let Some(source) = source_from(&source) else {
        return error(StatusCode::NOT_FOUND, "Unbekannte Quelle");
    };
    Json(state.webhook_test(source).await).into_response()
}

fn test_answer(ok: bool, message: &str) -> WebhookTestResponse {
    WebhookTestResponse {
        ok,
        message: message.into(),
    }
}

impl AppState {
    async fn webhook_test(&self, source: Source) -> WebhookTestResponse {
        if !self.inner.webhooks.settings().enabled {
            return test_answer(
                false,
                "Webhooks sind noch nicht aktiviert. Schalte sie oben ein.",
            );
        }
        let s = self.clone();
        let secret = tokio::task::spawn_blocking(move || s.token(secret_name(source)))
            .await
            .ok()
            .flatten();
        let Some(secret) = secret else {
            return test_answer(false, "Es gibt noch kein Secret. Erzeuge zuerst eines.");
        };
        // The body differs every time so that the replay check does not swallow a second test.
        let nonce = format!("{}-{}", now_ms(), std::process::id());
        let (path, body, headers) = match source {
            Source::GitHub => (
                GITHUB_PATH,
                format!(r#"{{"zen":"cockpit local test","nonce":"{nonce}"}}"#),
                "X-GitHub-Event: ping\r\n",
            ),
            Source::Jira => (
                JIRA_PATH,
                format!(r#"{{"webhookEvent":"cockpit:test","nonce":"{nonce}"}}"#),
                "",
            ),
        };
        let signature = signature_header(secret.expose().as_bytes(), body.as_bytes());
        let signature_name = match source {
            Source::GitHub => "X-Hub-Signature-256",
            Source::Jira => "X-Hub-Signature",
        };
        let port = self.inner.port;
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{signature_name}: {signature}\r\n{headers}X-Cockpit-Test: 1\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        match local_status(port, &request).await {
            Some(200) => test_answer(
                true,
                "Das Secret stimmt und die Route antwortet. Das ist ein lokaler Test: ob GitHub bzw. Jira dich über den Tunnel erreicht, zeigt erst „Zuletzt empfangen“ nach einer echten Zustellung.",
            ),
            Some(401) => test_answer(
                false,
                "Die Signatur wurde abgelehnt – das Secret passt nicht.",
            ),
            Some(404) => test_answer(
                false,
                "Die Route antwortet nicht. Sind Webhooks aktiviert und ein Secret gesetzt?",
            ),
            Some(429) => test_answer(
                false,
                "Zu viele Anfragen in kurzer Zeit. Warte eine Minute.",
            ),
            Some(_) => test_answer(false, "Der Server hat den Test abgelehnt."),
            None => test_answer(false, "Der lokale Server hat nicht geantwortet."),
        }
    }
}

/// Sends a raw HTTP/1.1 request to `127.0.0.1:port` and returns the status code.
async fn local_status(port: u16, request: &str) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let call = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .ok()?;
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut answer = Vec::new();
        let mut buf = [0u8; 512];
        // The status line is enough.
        while !answer.windows(2).any(|w| w == b"\r\n") && answer.len() < 4096 {
            let n = stream.read(&mut buf).await.ok()?;
            if n == 0 {
                break;
            }
            answer.extend_from_slice(&buf[..n]);
        }
        let line = String::from_utf8_lossy(&answer);
        line.lines().next()?.split_whitespace().nth(1)?.parse().ok()
    };
    tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests;
