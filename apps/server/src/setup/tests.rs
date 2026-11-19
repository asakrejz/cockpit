use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use cockpit_core::api::{ClientInput, EndpointInput, GitHubInput, JiraInput, Provider};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::app::{Settings, router};
use crate::secrets::{MemoryStore, SecretStore};
use crate::sync::Sources;

const WITH_COMMENTS: &str = "# Meine Einstellungen\nport = 8787 # Standard\n\n[github]\n# Diese Repos gleicht Cockpit ab\nrepos = [\"alt/repo\"] # alt\n";

fn change_github(repos: &[&str]) -> ConfigChange {
    ConfigChange {
        github: Some(GitHubInput {
            repos: repos.iter().map(|r| (*r).to_string()).collect(),
        }),
        ..Default::default()
    }
}

struct TestBench {
    app: Router,
    secrets: Arc<MemoryStore>,
    path: PathBuf,
    _dir: tempfile::TempDir,
}

fn test_bench(text: Option<&str>) -> TestBench {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    if let Some(t) = text {
        std::fs::write(&path, t).unwrap();
    }
    let secrets = Arc::new(MemoryStore::new());
    let e = Settings {
        search_path: Some(std::ffi::OsString::new()),
        config_path: Some(path.clone()),
        ..Settings::default()
    };
    let state = AppState::create(
        db::open_in_memory().unwrap(),
        secrets.clone(),
        8787,
        Sources::default(),
        e,
    );
    TestBench {
        app: router(state, PathBuf::from("/x")),
        secrets,
        path,
        _dir: dir,
    }
}

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
}

async fn send_request(app: &Router, req: Request<Body>) -> (StatusCode, String) {
    let answer = app.clone().oneshot(req).await.unwrap();
    let status = answer.status();
    let body = answer.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn put(app: &Router, path: &str, json: &str) -> (StatusCode, String) {
    send_request(
        app,
        request("PUT", path)
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
    )
    .await
}

async fn get(app: &Router, path: &str) -> serde_json::Value {
    let (status, body) = send_request(app, request("GET", path).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

// ---- Connection test for Jira (wiremock only, never a real service) ----

use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::config::JiraConfig;

mod access;
mod change;
mod github;
mod jira;
mod messages;
mod repos;
mod routes;

const TOKEN: &str = "test-wert-123";

/// A server whose Jira address is `site`; the token is only in the .env file on request.
fn jira_state(site: &str, email: &str, with_token: bool) -> (Router, Arc<MemoryStore>) {
    let secrets = Arc::new(MemoryStore::new());
    if with_token {
        secrets.set("jira", &Secret::new(TOKEN)).unwrap();
    }
    let sources = Sources {
        jira: (!site.is_empty()).then(|| JiraConfig {
            site: site.into(),
            email: email.into(),
            projects: vec![],
            jql: None,
            hidden_fields: vec![],
            field_order: vec![],
            team_field: None,
        }),
        ..Sources::default()
    };
    let state = AppState::create(
        db::open_in_memory().unwrap(),
        secrets.clone(),
        8787,
        sources,
        Settings::default(),
    );
    (router(state, PathBuf::from("/x")), secrets)
}

async fn check(app: &Router) -> serde_json::Value {
    let (status, body) = send_request(
        app,
        request("POST", "/api/jira/check")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(TOKEN), "token in the response: {body}");
    serde_json::from_str(&body).unwrap()
}

// ---------- Repos of the agent ----------

fn agent_repos_change() -> ConfigChange {
    use cockpit_core::AgentRepo;
    use cockpit_core::api::AgentReposInput;
    ConfigChange {
        agent_repos: Some(AgentReposInput {
            repos: vec![
                AgentRepo::new("firma/app"),
                AgentRepo {
                    base_branch: "develop".into(),
                    projects: vec!["WEB".into()],
                    ..AgentRepo::new("firma/web")
                },
            ],
            default: Some("firma/app".into()),
        }),
        ..Default::default()
    }
}
