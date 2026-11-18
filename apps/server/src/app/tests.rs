use super::*;
use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use cockpit_core::Actor;
use cockpit_integrations::Secret;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::config::EndpointConfig;
use crate::secrets::MemoryStore;
use cockpit_core::api::Provider;

fn state(secrets: Arc<MemoryStore>) -> AppState {
    let conn = db::open_in_memory().unwrap();
    db::connection_set(&conn, "github", "https://api.github.com", None).unwrap();
    db::connection_set(&conn, "jira", "https://x.atlassian.net", Some("a@b.de")).unwrap();
    AppState::new(conn, secrets)
}

/// A request as the browser sends it from `127.0.0.1:8787`.
fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
}

async fn send_request(app: Router, req: Request<Body>) -> (StatusCode, String) {
    let answer = app.oneshot(req).await.unwrap();
    let status = answer.status();
    let body = answer.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn hole(app: Router, path: &str) -> (StatusCode, String) {
    send_request(app, request("GET", path).body(Body::empty()).unwrap()).await
}

async fn send(app: Router, json: &str) -> (StatusCode, String) {
    let req = request("POST", "/api/actions")
        .header("content-type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap();
    send_request(app, req).await
}

fn app_without_dist() -> Router {
    router(
        state(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    )
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

/// Internal ID of the first entry of the sample data.
fn first_id() -> String {
    cockpit_core::demo::workspace().items[0].id.clone()
}

fn mark_done(id: &str) -> String {
    format!(r#"{{"action":{{"MarkDone":{{"item":"{id}"}}}}}}"#)
}

/// Reads from the event stream until `n` events (`data:` lines) are there.
async fn read_events(body: Body, n: usize) -> Vec<serde_json::Value> {
    use http_body_util::BodyExt;
    let mut body = body;
    let mut text = String::new();
    let mut data = Vec::new();
    while data.len() < n {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("event did not arrive")
            .expect("stream ended")
            .unwrap();
        if let Some(chunk) = frame.data_ref() {
            text.push_str(&String::from_utf8_lossy(chunk));
        }
        data = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|l| json(l.trim()))
            .collect();
    }
    data
}

fn state_with(conn: Connection, e: Settings, secrets: Arc<MemoryStore>) -> AppState {
    AppState::create(conn, secrets, 8787, Sources::default(), e)
}

fn empty() -> Settings {
    Settings {
        search_path: Some(std::ffi::OsString::new()),
        ..Settings::default()
    }
}

async fn state_from(state: AppState) -> (cockpit_core::Workspace, u64) {
    let app = router(state, PathBuf::from("/x"));
    let (status, body) = hole(app, "/api/workspace").await;
    assert_eq!(status, StatusCode::OK);
    cockpit_core::persist::load_with_revision(&body).unwrap()
}

/// `url = None`: directly at the provider, then the call goes to `anthropic_api` (the fake).
fn with_endpoint(
    url: Option<&str>,
    anthropic_api: Option<&str>,
    secrets: Arc<MemoryStore>,
) -> Router {
    let e = Settings {
        agent: AgentChoice {
            provider: Provider::ClaudeAgentSdk,
            endpoint: Some(EndpointConfig {
                url: url.map(str::to_string),
                model: Some("claude-sonnet-5-5".into()),
            }),
        },
        anthropic_api: anthropic_api.map(str::to_string),
        ..empty()
    };
    let conn = db::open_in_memory().unwrap();
    router(state_with(conn, e, secrets), PathBuf::from("/x"))
}

mod actions;
mod basics;
mod endpoint;
mod events;
mod persistence;
mod security;
mod seed;
mod tick;
mod translate;
