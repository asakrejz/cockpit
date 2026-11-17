use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use cockpit_integrations::oauth::Endpoints;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::app::router;
use crate::config::{AtlassianConfig, GitHubConfig, JiraConfig, OAuthConfig};
use crate::secrets::{MemoryStore, SecretStore};
use crate::sync::Sources;

mod atlassian;
mod callback;
mod refresh;
mod revoke;
mod sign_in;

const CLIENT_SECRET: &str = "client-geheimnis-abc123";

fn endpoints(s: &MockServer) -> Endpoints {
    Endpoints {
        github_sign_in: format!("{}/login/oauth/authorize", s.uri()),
        github_token: format!("{}/login/oauth/access_token", s.uri()),
        atlassian_sign_in: format!("{}/authorize", s.uri()),
        atlassian_token: format!("{}/oauth/token", s.uri()),
        atlassian_api: s.uri(),
    }
}

struct TestBench {
    state: AppState,
    app: Router,
    secrets: Arc<MemoryStore>,
}

fn snapshot(server: &MockServer, site: Option<&str>, with_secret: bool) -> TestBench {
    let secrets = Arc::new(MemoryStore::new());
    if with_secret {
        for n in [secrets::GITHUB_OAUTH, secrets::ATLASSIAN_OAUTH] {
            secrets.set(n, &Secret::new(CLIENT_SECRET)).unwrap();
        }
    }
    let sources = Sources {
        github: Some(GitHubConfig {
            api_url: server.uri(),
            repos: vec!["a/x".into()],
        }),
        jira: Some(JiraConfig {
            site: String::new(),
            email: String::new(),
            projects: vec![],
            jql: None,
        }),
        github_oauth: Some(OAuthConfig {
            client_id: "gh-id".into(),
        }),
        atlassian: Some(AtlassianConfig {
            client_id: "atl-id".into(),
            site: site.map(Into::into),
        }),
        endpoints: endpoints(server),
        ..Sources::default()
    };
    let state = AppState::with_sources(
        crate::db::open_in_memory().unwrap(),
        secrets.clone(),
        8787,
        sources,
    );
    TestBench {
        app: router(state.clone(), PathBuf::from("/gibt/es/nicht")),
        state,
        secrets,
    }
}

async fn call(app: &Router, method: &str, path: &str) -> (StatusCode, String, String) {
    // Like the navigation of the browser: no content type, no origin.
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8787")
        .header("content-type", "application/json")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
        .body(Body::empty())
        .unwrap();
    let r = app.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let ort = r
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, ort, String::from_utf8_lossy(&body).into_owned())
}

/// A value from the query of an address (percent-decoded).
fn param(url: &str, name: &str) -> String {
    let query = url
        .split_once('?')
        .map_or("", |(_, q)| q.split('#').next().unwrap_or(""));
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| decoded(v))
        .unwrap_or_default()
}

fn decoded(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                out.push(u8::from_str_radix(h, 16).unwrap_or(b'?'));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn token_mock_github(server: &MockServer, expected: u64) {
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ghu_zugriff_geheim", "expires_in": 28800,
            "refresh_token": "ghr_erneuern_geheim"
        })))
        .expect(expected)
        .mount(server)
        .await;
}

async fn github_state(p: &TestBench) -> String {
    let (status, ort, _) = call(&p.app, "GET", "/api/oauth/github/start").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(ort.contains("/login/oauth/authorize?"), "{ort}");
    assert_eq!(param(&ort, "client_id"), "gh-id");
    assert_eq!(param(&ort, "code_challenge_method"), "S256");
    assert!(!param(&ort, "code_challenge").is_empty());
    assert_eq!(
        param(&ort, "redirect_uri"),
        "http://127.0.0.1:8787/api/oauth/github/callback"
    );
    assert!(!ort.contains(CLIENT_SECRET));
    param(&ort, "state")
}

async fn atlassian_mocks(server: &MockServer, sites: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "atl_zugriff_geheim", "expires_in": 3600,
            "refresh_token": "atl_erneuern_geheim"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/oauth/token/accessible-resources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sites))
        .mount(server)
        .await;
}

async fn atlassian_sign_in(p: &TestBench) -> String {
    let (_, ort, _) = call(&p.app, "GET", "/api/oauth/atlassian/start").await;
    assert!(ort.contains("/authorize?"), "{ort}");
    assert_eq!(param(&ort, "audience"), "api.atlassian.com");
    assert!(param(&ort, "scope").contains("offline_access"));
    assert_eq!(
        param(&ort, "redirect_uri"),
        "http://localhost:8787/api/oauth/atlassian/callback"
    );
    let state = param(&ort, "state");
    call(
        &p.app,
        "GET",
        &format!("/api/oauth/atlassian/callback?code=c&state={state}"),
    )
    .await
    .1
}
