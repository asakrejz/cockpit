use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::app::{Settings, router};
use crate::config::{GitHubConfig, JiraConfig};
use crate::db;
use crate::secrets::{MemoryStore, SecretStore};
use crate::sync::Sources;

mod assistant;
mod debounce;
mod host;
mod routes;
mod signature;

const GH_SECRET: &str = "dummy-github-webhook-secret";
const JIRA_SECRET: &str = "dummy-jira-webhook-secret";

// ---- the routes ----------------------------------------------------------------------------

struct Fixture {
    state: AppState,
    github: MockServer,
    jira: MockServer,
    secrets: Arc<MemoryStore>,
}

async fn mocks() -> (MockServer, MockServer) {
    let g = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "ich"})))
        .mount(&g)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": []})))
        .mount(&g)
        .await;
    let j = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accountId": "ich"})))
        .mount(&j)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": []})))
        .mount(&j)
        .await;
    (g, j)
}

async fn fixture(enabled: bool, with_secrets: bool, debounce: Duration, port: u16) -> Fixture {
    let (github, jira) = mocks().await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(crate::secrets::GITHUB, &Secret::new("dummy-gh-token"))
        .unwrap();
    secrets
        .set(crate::secrets::JIRA, &Secret::new("dummy-jira-token"))
        .unwrap();
    if with_secrets {
        secrets
            .set(crate::secrets::GITHUB_WEBHOOK, &Secret::new(GH_SECRET))
            .unwrap();
        secrets
            .set(crate::secrets::JIRA_WEBHOOK, &Secret::new(JIRA_SECRET))
            .unwrap();
    }
    let sources = Sources {
        github: Some(GitHubConfig {
            api_url: github.uri(),
            repos: vec!["a/x".into()],
        }),
        jira: Some(JiraConfig {
            site: jira.uri(),
            email: "ich@firma.example".into(),
            projects: vec!["WEB".into()],
            jql: None,
        }),
        ..Sources::default()
    };
    let settings = Settings {
        search_path: Some(std::ffi::OsString::new()),
        webhooks: WebhookSettings {
            enabled,
            debounce,
            ..Default::default()
        },
        ..Settings::default()
    };
    let state = AppState::create(
        db::open_in_memory().unwrap(),
        secrets.clone(),
        port,
        sources,
        settings,
    );
    Fixture {
        state,
        github,
        jira,
        secrets,
    }
}

async fn plain() -> Fixture {
    fixture(true, true, Duration::from_millis(50), 8787).await
}

async fn count(server: &MockServer, p: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == p)
        .count()
}

/// Waits until `p` was requested `n` times (or fails after three seconds).
async fn wait_for(server: &MockServer, p: &str, n: usize) {
    for _ in 0..300 {
        if count(server, p).await >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{p} was not requested {n} times");
}

type Headers = Vec<(&'static str, String)>;

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    host: &str,
    headers: &Headers,
    body: Vec<u8>,
) -> (StatusCode, String) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host)
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    let app: Router = router(state.clone(), PathBuf::from("/x"));
    let answer = app
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = answer.status();
    let body = answer.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn gh_headers(body: &[u8], event: &str, delivery: &str) -> Headers {
    vec![
        ("content-type", "application/json".into()),
        ("x-github-event", event.into()),
        ("x-github-delivery", delivery.into()),
        (
            "x-hub-signature-256",
            signature_header(GH_SECRET.as_bytes(), body),
        ),
    ]
}

async fn gh(f: &Fixture, event: &str, delivery: &str, body: &str) -> StatusCode {
    let h = gh_headers(body.as_bytes(), event, delivery);
    call(
        &f.state,
        "POST",
        GITHUB_PATH,
        "cockpit.example",
        &h,
        body.as_bytes().to_vec(),
    )
    .await
    .0
}

async fn jira(f: &Fixture, body: &str) -> StatusCode {
    let h: Headers = vec![
        ("content-type", "application/json".into()),
        (
            "x-hub-signature",
            signature_header(JIRA_SECRET.as_bytes(), body.as_bytes()),
        ),
    ];
    call(
        &f.state,
        "POST",
        JIRA_PATH,
        "cockpit.example",
        &h,
        body.as_bytes().to_vec(),
    )
    .await
    .0
}
