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

#[test]
fn change_keeps_comments_and_other_values() {
    let (updated, c) = change(WITH_COMMENTS, &change_github(&["firma/app", "firma/web"])).unwrap();
    assert!(updated.contains("# Meine Einstellungen"), "{updated}");
    assert!(updated.contains("port = 8787 # Standard"), "{updated}");
    assert!(
        updated.contains("# Diese Repos gleicht Cockpit ab"),
        "{updated}"
    );
    assert!(updated.contains("# alt"), "{updated}");
    assert_eq!(c.github.unwrap().repos, vec!["firma/app", "firma/web"]);
}

#[test]
fn change_puts_sections_in_a_empty_file_at() {
    let a = ConfigChange {
        atlassian: Some(ClientInput {
            client_id: " 7dNimh ".into(),
        }),
        github_oauth: Some(ClientInput {
            client_id: "Iv23li".into(),
        }),
        jira: Some(JiraInput {
            projects: Some(vec!["bau".into(), "web".into()]),
            ..Default::default()
        }),
        endpoint: Some(EndpointInput {
            provider: Some(Provider::ClaudeAgentSdk),
            url: Some("localhost:4000/".into()),
            model: Some("claude-sonnet-5-5".into()),
        }),
        ..Default::default()
    };
    let (updated, c) = change("", &a).unwrap();
    assert_eq!(c.atlassian.unwrap().client_id, "7dNimh");
    assert_eq!(c.github_oauth.unwrap().client_id, "Iv23li");
    assert_eq!(c.jira.unwrap().projects, vec!["BAU", "WEB"]);
    let l = c.endpoint.unwrap();
    assert_eq!(l.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(l.model.as_deref(), Some("claude-sonnet-5-5"));
    // The new format, no `[litellm]`.
    assert!(
        updated.contains("[agent.endpoint]") && !updated.contains("litellm"),
        "{updated}"
    );
    assert!(
        updated.contains("provider = \"claude-agent-sdk\""),
        "{updated}"
    );
    // The result is readable again as at start.
    assert!(Config::parse(Some(&updated), |_| None).is_ok());
}

#[test]
fn change_adopts_a_older_litellm_and_keeps_agent_values() {
    let alt = "[litellm]\nurl = \"http://a:1\"\nmodell = \"altes-modell\"\n[agent]\ncontainer = \"podman\" # meins\nrepo = \"firma/app\"\n";
    // Only change the model: the old address stays, `[litellm]` moves.
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            model: Some("neues-modell".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (updated, c) = change(alt, &a).unwrap();
    assert!(!updated.contains("[litellm]"), "{updated}");
    assert!(
        updated.contains("container = \"podman\" # meins"),
        "{updated}"
    );
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://a:1"));
    assert_eq!(e.model.as_deref(), Some("neues-modell"));
    assert_eq!(c.container.as_deref(), Some("podman"));
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
}

#[test]
fn change_migrates_all_older_spellings_to_the_current_ones() {
    // `[agent] anbieter`, `[agent.endpunkt]` with `modell`, plus a `[litellm]` that loses.
    let old = "[litellm]\nurl = \"http://x:9\"\nmodell = \"litellm-model\"\n[agent]\nanbieter = \"codex\"\n[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n";
    let a = ConfigChange {
        endpoint: Some(EndpointInput::default()),
        ..Default::default()
    };
    let (updated, c) = change(old, &a).unwrap();
    for gone in ["litellm", "anbieter", "endpunkt", "modell"] {
        assert!(!updated.contains(gone), "{gone} in {updated}");
    }
    assert!(updated.contains("provider = \"codex\""), "{updated}");
    assert!(updated.contains("[agent.endpoint]"), "{updated}");
    let e = c.endpoint.unwrap();
    assert_eq!(c.provider, Provider::Codex);
    assert_eq!(
        (e.url.as_deref(), e.model.as_deref()),
        (Some("http://a:1"), Some("m"))
    );
    // The result is stable: a second change does not move anything.
    assert_eq!(change(&updated, &a).unwrap().0, updated);
}

#[test]
fn change_switches_on_direct_without_address() {
    let start = "[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n";
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            provider: Some(Provider::ClaudeAgentSdk),
            url: Some(String::new()),
            model: Some("claude-opus-5-5".into()),
        }),
        ..Default::default()
    };
    let (updated, c) = change(start, &a).unwrap();
    assert!(!updated.contains("url"), "{updated}");
    let e = c.endpoint.unwrap();
    assert_eq!((e.url, e.model.as_deref()), (None, Some("claude-opus-5-5")));
    // A tool that is not implemented yet can be entered.
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            provider: Some(Provider::Codex),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(change("", &a).unwrap().1.provider, Provider::Codex);
}

#[test]
fn change_removes_with_empty_evaluate() {
    let start = "[atlassian]\nclient_id = \"x\"\n[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n[github]\nrepos = [\"a/b\"]\n";
    let a = ConfigChange {
        atlassian: Some(ClientInput::default()),
        endpoint: Some(EndpointInput {
            url: Some(String::new()),
            model: Some(String::new()),
            ..Default::default()
        }),
        github: Some(GitHubInput::default()),
        ..Default::default()
    };
    let (updated, c) = change(start, &a).unwrap();
    assert!(c.atlassian.is_none() && c.endpoint.is_none());
    assert!(!updated.contains("agent"), "{updated}");
    assert!(c.github.is_none(), "{updated}");
}

#[test]
fn change_checks_like_the_start() {
    // Jira without OAuth needs site and email.
    let only_projects = ConfigChange {
        jira: Some(JiraInput {
            projects: Some(vec!["BAU".into()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(change("", &only_projects).is_err());
    let token = ConfigChange {
        jira: Some(JiraInput {
            site: Some("firma".into()),
            email: Some("ich@firma.example".into()),
            projects: None,
        }),
        ..Default::default()
    };
    let (_, c) = change("", &token).unwrap();
    let j = c.jira.unwrap();
    assert_eq!(j.site, "https://firma.atlassian.net");
    assert_eq!(j.email, "ich@firma.example");

    for bad in [
        change_github(&["kein-repo"]),
        ConfigChange {
            jira: Some(JiraInput {
                email: Some("keine-adresse".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ConfigChange {
            endpoint: Some(EndpointInput {
                url: Some("ftp://x".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ConfigChange {
            github_oauth: Some(ClientInput {
                client_id: "mit leerzeichen".into(),
            }),
            ..Default::default()
        },
    ] {
        assert!(change("", &bad).is_err(), "{bad:?}");
    }
    // A broken file is not overwritten.
    assert!(change("port = [", &change_github(&["a/b"])).is_err());
}

#[test]
fn write_is_atomic_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unter").join("config.toml");
    atomic_write(&path, "a = 1\n").unwrap();
    atomic_write(&path, "a = 2\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
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

#[tokio::test]
async fn key_lands_in_env_file_and_comes_never_back() {
    let p = test_bench(None);
    let (status, body) = put(
        &p.app,
        "/api/secrets/github-oauth",
        r#"{"value":"  test-wert-123 "}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("test-wert-123"), "{body}");
    assert!(body.contains(r#""set":true"#));
    assert_eq!(
        p.secrets.get("github-oauth").unwrap().unwrap().expose(),
        "test-wert-123"
    );
    // Neither the setup nor the health report show the value.
    let e = get(&p.app, "/api/setup").await;
    assert!(!e.to_string().contains("test-wert-123"));
    let flag = |n: &str| {
        e["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == n)
            .unwrap()["set"]
            .as_bool()
            .unwrap()
    };
    assert!(flag("github-oauth") && !flag("atlassian-oauth"));
    let h = get(&p.app, "/api/health").await;
    assert!(!h.to_string().contains("test-wert-123"));
}

#[tokio::test]
async fn key_only_with_allowed_names_and_valid_value() {
    let p = test_bench(None);
    for name in ["github-oauth-token", "unsinn", "Jira"] {
        let (status, body) = put(
            &p.app,
            &format!("/api/secrets/{name}"),
            r#"{"value":"test-wert-123"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{name}");
        assert!(!body.contains("test-wert-123"));
    }
    assert!(p.secrets.get("github-oauth-token").unwrap().is_none());
    for (body, expected) in [
        (r#"{"value":""}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (r#"{"value":"  "}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (
            r#"{"value":"test-wert\n123"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("kein json test-wert-123", StatusCode::BAD_REQUEST),
        (r#"{"anders":"test-wert-123"}"#, StatusCode::BAD_REQUEST),
    ] {
        let (status, answer) = put(&p.app, "/api/secrets/litellm", body).await;
        assert_eq!(status, expected, "{body}");
        assert!(!answer.contains("test-wert"), "{answer}");
    }
    assert!(p.secrets.get("litellm").unwrap().is_none());
}

#[tokio::test]
async fn new_routes_have_same_checks_like_all_api_routes() {
    let p = test_bench(None);
    let value = r#"{"value":"test-wert-123"}"#;
    // Content-Type muss JSON sein.
    for kind in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut b = request("PUT", "/api/secrets/litellm");
        if let Some(t) = kind {
            b = b.header("content-type", t);
        }
        let (status, _) = send_request(&p.app, b.body(Body::from(value)).unwrap()).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    // Foreign origin, foreign host, foreign peer.
    let (status, _) = send_request(
        &p.app,
        request("PUT", "/api/secrets/litellm")
            .header("content-type", "application/json")
            .header("origin", "https://boese.example")
            .body(Body::from(value))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_request(
        &p.app,
        Request::builder()
            .method("PUT")
            .uri("/api/secrets/litellm")
            .header("host", "boese.example")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::from(value))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_request(
        &p.app,
        Request::builder()
            .method("PUT")
            .uri("/api/config")
            .header("host", "127.0.0.1:8787")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))))
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_request(
        &p.app,
        Request::builder()
            .uri("/api/setup")
            .header("host", "boese.example")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(p.secrets.get("litellm").unwrap().is_none());
    assert!(!p.path.exists());
}

#[tokio::test]
async fn the_agent_key_goes_in_the_env_file_and_never_back() {
    let p = test_bench(None);
    let (status, answer) = put(
        &p.app,
        "/api/secrets/agent-key",
        r#"{"value":"sk-test-123"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(!answer.contains("sk-test-123"), "{answer}");
    assert!(p.secrets.get("agent-key").unwrap().is_some());
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpoint"]["key"], true);
    assert!(!h.to_string().contains("sk-test-123"));
    let e = get(&p.app, "/api/setup").await;
    assert!(!e.to_string().contains("sk-test-123"));
}

#[tokio::test]
async fn config_writes_the_file_and_takes_effect_without_restart() {
    let p = test_bench(Some(WITH_COMMENTS));
    // Before: nothing set up.
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpoint"]["configured"], false);

    let body = r#"{"endpoint":{"provider":"claude-agent-sdk","url":"localhost:4000","model":"claude-sonnet-5-5"},
        "github_oauth":{"client_id":"Iv23li"},
        "github":{"repos":["firma/app","https://github.com/firma/web.git"]}}"#;
    let (status, answer) = put(&p.app, "/api/config", body).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let a: ConfigResponse = serde_json::from_str(&answer).unwrap();
    assert!(a.stored && a.takes_effect_immediately);

    // File: new, but comments and old values stay.
    let file = std::fs::read_to_string(&p.path).unwrap();
    assert!(file.contains("# Meine Einstellungen") && file.contains("port = 8787 # Standard"));
    assert!(
        file.contains("firma/web") && !file.contains("alt/repo"),
        "{file}"
    );
    assert!(Config::parse(Some(&file), |_| None).is_ok());

    // Visible without restart.
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpoint"]["url"], "http://localhost:4000");
    assert_eq!(h["endpoint"]["model"], "claude-sonnet-5-5");
    // The older names of the request (`litellm`) still apply too.
    let (status, answer) = put(
        &p.app,
        "/api/config",
        r#"{"litellm":{"url":"localhost:4001"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let e = get(&p.app, "/api/setup").await;
    assert_eq!(e["endpoint_url"], "http://localhost:4001");
    assert_eq!(e["endpoint_model"], "claude-sonnet-5-5");
    assert_eq!(e["provider"], "claude-agent-sdk");
    let e = get(&p.app, "/api/setup").await;
    assert_eq!(e["github_client_id"], "Iv23li");
    assert_eq!(
        e["github_repos"],
        serde_json::json!(["firma/app", "firma/web"])
    );
    assert_eq!(
        e["callback_github"],
        "http://127.0.0.1:8787/api/oauth/github/callback"
    );
    assert_eq!(
        e["callback_atlassian"],
        "http://localhost:8787/api/oauth/atlassian/callback"
    );
    // With client ID and secret "Mit GitHub anmelden" is possible.
    put(
        &p.app,
        "/api/secrets/github-oauth",
        r#"{"value":"test-wert-123"}"#,
    )
    .await;
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["sign_in"]["github"]["oauth_possible"], true);
}

#[tokio::test]
async fn invalid_inputs_change_nothing_and_say_why() {
    let p = test_bench(Some(WITH_COMMENTS));
    let (status, body) = put(
        &p.app,
        "/api/config",
        r#"{"github":{"repos":["kein-repo"]}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains("kein-repo") && body.contains("firma/app"),
        "{body}"
    );
    assert_eq!(std::fs::read_to_string(&p.path).unwrap(), WITH_COMMENTS);
    let (status, _) = put(&p.app, "/api/config", "kein json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn new_client_id_brings_the_hint_to_new_sign_in() {
    let p = test_bench(None);
    let id = |i: &str| format!(r#"{{"atlassian":{{"client_id":"{i}"}}}}"#);
    let (_, first) = put(&p.app, "/api/config", &id("alt")).await;
    assert!(
        serde_json::from_str::<ConfigResponse>(&first)
            .unwrap()
            .hint
            .is_none()
    );
    let (_, second) = put(&p.app, "/api/config", &id("neu")).await;
    assert!(
        serde_json::from_str::<ConfigResponse>(&second)
            .unwrap()
            .hint
            .is_some()
    );
}

#[tokio::test]
async fn without_file_is_it_created() {
    let p = test_bench(None);
    let (status, _) = put(
        &p.app,
        "/api/config",
        r#"{"jira":{"site":"firma","email":"ich@firma.example"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        std::fs::read_to_string(&p.path)
            .unwrap()
            .contains("https://firma.atlassian.net")
    );
    // Jira via token still needs the key: only after that "connected".
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["sign_in"]["jira"]["connected"], false);
    put(&p.app, "/api/secrets/jira", r#"{"value":"test-wert-123"}"#).await;
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["sign_in"]["jira"]["connected"], true);
}

// ---- Connection test for Jira (wiremock only, never a real service) ----

use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::config::JiraConfig;

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
    assert!(!body.contains(TOKEN), "Token in der Antwort: {body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn jira_test_ok_reads_only_and_knows_the_names() {
    let jira = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .and(header_exists("authorization"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"accountId":"acc-1","displayName":"Mira Keller"}"#),
        )
        .mount(&jira)
        .await;
    let (app, _) = jira_state(&jira.uri(), "mira@firma.example", true);
    let a = check(&app).await;
    assert_eq!(a["ok"], true);
    assert_eq!(a["cause"], "ok");
    assert_eq!(a["name"], "Mira Keller");
    assert!(a["message"].as_str().unwrap().contains("kennt dich"));
    // Exactly one reading call, nothing else.
    let requests = jira.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "GET");
    assert_eq!(requests[0].url.path(), "/rest/api/3/myself");
}

#[tokio::test]
async fn jira_test_says_in_plain_text_what_it_read() {
    for (answer, cause) in [
        (ResponseTemplate::new(401), "token"),
        (ResponseTemplate::new(403), "permission"),
        (ResponseTemplate::new(404), "address"),
        // A foreign page instead of Jira.
        (
            ResponseTemplate::new(200).set_body_string("<html>Anmelden</html>"),
            "address",
        ),
        (ResponseTemplate::new(500), "other"),
    ] {
        let jira = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/myself"))
            .respond_with(answer)
            .mount(&jira)
            .await;
        let (app, _) = jira_state(&jira.uri(), "mira@firma.example", true);
        let a = check(&app).await;
        assert_eq!(a["ok"], false);
        assert_eq!(a["cause"], cause, "{a}");
        assert!(!a["message"].as_str().unwrap().is_empty());
    }
}

#[tokio::test]
async fn jira_test_without_connection_and_without_details() {
    // Nothing listens on the port.
    let (app, _) = jira_state("http://127.0.0.1:1", "mira@firma.example", true);
    assert_eq!(check(&app).await["cause"], "reachable");
    // Without token, without email, without site: "details", without anything being called.
    for (site, email, token) in [
        ("http://127.0.0.1:1", "mira@firma.example", false),
        ("http://127.0.0.1:1", "", true),
        ("", "", true),
    ] {
        let (app, _) = jira_state(site, email, token);
        let a = check(&app).await;
        assert_eq!(a["cause"], "details", "{site} {email} {token}");
        assert_eq!(a["ok"], false);
    }
}

#[tokio::test]
async fn jira_test_gives_the_token_never_out_also_when_jira_it_echoes() {
    let jira = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string(format!("Bad {TOKEN}")))
        .mount(&jira)
        .await;
    let (app, _) = jira_state(&jira.uri(), "mira@firma.example", true);
    let a = check(&app).await;
    assert_eq!(a["ok"], false);
    // `check` already checks that the token is not in the answer.
    assert!(!a.to_string().contains(TOKEN));
}

#[tokio::test]
async fn disconnect_deletes_only_tokens() {
    let (app, secrets) = jira_state("", "", true);
    secrets.set("litellm", &Secret::new(TOKEN)).unwrap();
    let delete = |name: &str| {
        request("DELETE", &format!("/api/secrets/{name}"))
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap()
    };
    let (status, body) = send_request(&app, delete("jira")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""set":false"#) && !body.contains(TOKEN));
    assert!(secrets.get("jira").unwrap().is_none());
    // A second time is no error.
    assert_eq!(send_request(&app, delete("jira")).await.0, StatusCode::OK);
    // The model key and client secrets stay.
    for name in ["litellm", "atlassian-oauth", "unsinn"] {
        assert_eq!(
            send_request(&app, delete(name)).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert!(secrets.get("litellm").unwrap().is_some());
    // Without a JSON content type the shared check rejects.
    let raw = request("DELETE", "/api/secrets/jira")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send_request(&app, raw).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}
