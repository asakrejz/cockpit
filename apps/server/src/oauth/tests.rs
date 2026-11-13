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

#[tokio::test]
async fn start_without_setup_shows_a_understandable_message() {
    let server = MockServer::start().await;
    // Without a client secret in the .env file.
    let p = snapshot(&server, None, false);
    let (status, ort, _) = call(&p.app, "GET", "/api/oauth/github/start").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        ort.starts_with("http://127.0.0.1:8787/?oauth=error&service=github"),
        "{ort}"
    );
    let message = param(&ort, "message");
    assert!(message.contains("token setzen github-oauth"), "{message}");

    // Without a client ID in the configuration.
    let p = TestBench {
        state: AppState::new(
            crate::db::open_in_memory().unwrap(),
            Arc::new(MemoryStore::new()),
        ),
        app: Router::new(),
        secrets: Arc::new(MemoryStore::new()),
    };
    let _ = &p.secrets;
    assert!(p.state.oauth_start(Service::Atlassian).await.is_err());

    // Unknown service.
    let p = snapshot(&server, None, true);
    let (status, _, _) = call(&p.app, "GET", "/api/oauth/gitlab/start").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn github_sign_in_from_start_until_health() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await;
    let p = snapshot(&server, None, true);
    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["github"]["connected"], false);
    assert_eq!(h["sign_in"]["github"]["oauth_possible"], true);

    let state = github_state(&p).await;
    let (status, ort, body) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=abc&state={state}&installation_id=5&setup_action=install"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        ort.starts_with("http://127.0.0.1:8787/?oauth=ok&service=github"),
        "{ort}"
    );
    assert!(ort.ends_with("#/setup"));
    // Nothing secret in redirect or body.
    for text in [&ort, &body] {
        for secret in ["ghu_zugriff_geheim", "ghr_erneuern_geheim", CLIENT_SECRET] {
            assert!(!text.contains(secret));
        }
    }
    // Tokens are in the .env file.
    let stored = p
        .secrets
        .get(secrets::GITHUB_OAUTH_TOKENS)
        .unwrap()
        .unwrap();
    let t = Tokens::from_text(stored.expose()).unwrap();
    assert_eq!(t.access.expose(), "ghu_zugriff_geheim");
    assert_eq!(t.refresh.unwrap().expose(), "ghr_erneuern_geheim");

    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["github"]["connected"], true);
    assert_eq!(h["sign_in"]["github"]["kind"], "oauth");
    assert_eq!(h["connections"]["github"], true);
    assert!(!h.to_string().contains("geheim"));
}

#[tokio::test]
async fn wrong_and_duplicate_state() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await; // exactly one exchange
    let p = snapshot(&server, None, true);

    // Wrong state: no exchange, nothing stored.
    let _ = github_state(&p).await;
    let (_, ort, _) = call(
        &p.app,
        "GET",
        "/api/oauth/github/callback?code=abc&state=falsch",
    )
    .await;
    assert!(ort.contains("oauth=error"), "{ort}");
    assert!(param(&ort, "message").contains("unbekannt"));
    // Without state (e.g. return from the installation without our start).
    let (_, ort, _) = call(
        &p.app,
        "GET",
        "/api/oauth/github/callback?code=abc&installation_id=5&setup_action=install",
    )
    .await;
    assert!(ort.contains("oauth=error"));
    assert!(param(&ort, "message").contains("Mit GitHub anmelden"));
    assert!(
        p.secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .is_none()
    );

    // Right, then the same callback once more.
    let state = github_state(&p).await;
    let url = format!("/api/oauth/github/callback?code=abc&state={state}");
    assert!(call(&p.app, "GET", &url).await.1.contains("oauth=ok"));
    let (_, ort, _) = call(&p.app, "GET", &url).await;
    assert!(ort.contains("oauth=error"), "{ort}");
    assert!(param(&ort, "message").contains("schon verwendet"));
    assert!(
        p.secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .is_some()
    );

    // State from GitHub, callback from Atlassian.
    let state = github_state(&p).await;
    let (_, ort, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/atlassian/callback?code=abc&state={state}"),
    )
    .await;
    assert!(param(&ort, "message").contains("anderen Dienst"), "{ort}");
}

#[tokio::test]
async fn error_the_service_and_cancel() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_verification_code",
            "error_description": "Der Code ist abgelaufen"
        })))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    let state = github_state(&p).await;
    let (_, ort, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=abc&state={state}"),
    )
    .await;
    assert!(ort.contains("oauth=error"));
    assert!(param(&ort, "message").contains("abgelaufen"));
    assert!(
        p.secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .is_none()
    );

    // The user declines.
    let state = github_state(&p).await;
    let (_, ort, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?error=access_denied&error_description=The+user+denied&state={state}"),
    )
    .await;
    assert!(
        param(&ort, "message").contains("abgebrochen oder abgelehnt: The user denied"),
        "{ort}"
    );
    // The state is consumed by that.
    let (_, ort, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=abc&state={state}"),
    )
    .await;
    assert!(param(&ort, "message").contains("schon verwendet"));
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

#[tokio::test]
async fn atlassian_a_site_is_automatic_chosen() {
    let server = MockServer::start().await;
    atlassian_mocks(
        &server,
        serde_json::json!([{"id":"cloud-1","name":"firma","url":"https://firma.atlassian.net","scopes":[]}]),
    )
    .await;
    let p = snapshot(&server, None, true);
    let ort = atlassian_sign_in(&p).await;
    assert!(
        ort.starts_with("http://127.0.0.1:8787/?oauth=ok&service=atlassian"),
        "{ort}"
    );
    assert!(param(&ort, "message").contains("firma"));
    assert_eq!(
        p.state.atlassian_site(),
        Some(("cloud-1".into(), "firma".into()))
    );
    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["jira"]["kind"], "oauth");
    assert_eq!(h["sign_in"]["jira"]["site"], "firma");
    assert_eq!(h["connections"]["jira"], true);
    // Jira is addressed through the gateway.
    let services = tokio::task::spawn_blocking({
        let s = p.state.clone();
        move || s.services_build()
    })
    .await
    .unwrap();
    assert!(services.jira.is_some());
}

#[tokio::test]
async fn atlassian_several_sites_need_a_choice() {
    let server = MockServer::start().await;
    atlassian_mocks(
        &server,
        serde_json::json!([
            {"id":"c1","name":"firma","url":"https://firma.atlassian.net","scopes":[]},
            {"id":"c2","name":"privat","url":"https://privat.atlassian.net","scopes":[]}
        ]),
    )
    .await;
    let p = snapshot(&server, None, true);
    let ort = atlassian_sign_in(&p).await;
    assert!(ort.contains("oauth=error"), "{ort}");
    let message = param(&ort, "message");
    assert!(
        message.contains("firma") && message.contains("privat"),
        "{message}"
    );
    // No half state.
    assert!(
        p.secrets
            .get(secrets::ATLASSIAN_OAUTH_TOKENS)
            .unwrap()
            .is_none()
    );
    assert_eq!(p.state.atlassian_site(), None);

    let p = snapshot(&server, Some("privat"), true);
    let ort = atlassian_sign_in(&p).await;
    assert!(ort.contains("oauth=ok"), "{ort}");
    assert_eq!(p.state.atlassian_site().unwrap().0, "c2");
}

#[tokio::test]
async fn revoke_github_calls_the_service_and_deletes() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await;
    Mock::given(method("DELETE"))
        .and(path("/applications/gh-id/token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    let state = github_state(&p).await;
    call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=c&state={state}"),
    )
    .await;
    assert!(p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());

    let (status, _, body) = call(&p.app, "POST", "/api/oauth/github/revoke").await;
    assert_eq!(status, StatusCode::OK);
    let a: RevokeResponse = serde_json::from_str(&body).unwrap();
    assert!(a.disconnected && a.at_service, "{body}");
    assert!(!p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());
    // The client secret stays (for the next sign-in).
    assert!(p.secrets.has(secrets::GITHUB_OAUTH).unwrap());
    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["github"]["connected"], false);
    assert_eq!(h["sign_in"]["github"]["kind"], "none");
}

#[tokio::test]
async fn revoke_disconnects_also_when_the_service_fails() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    let state = github_state(&p).await;
    call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=c&state={state}"),
    )
    .await;
    let a = p.state.oauth_revoke(Service::GitHub).await.unwrap();
    assert!(a.disconnected && !a.at_service);
    assert!(a.hint.unwrap().contains("nicht bestätigt"));
    assert!(!p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());
}

#[tokio::test]
async fn revoke_atlassian_deletes_tokens_and_site() {
    let server = MockServer::start().await;
    atlassian_mocks(
        &server,
        serde_json::json!([{"id":"c1","name":"firma","url":"https://firma.atlassian.net","scopes":[]}]),
    )
    .await;
    let p = snapshot(&server, None, true);
    atlassian_sign_in(&p).await;
    let (status, _, body) = call(&p.app, "POST", "/api/oauth/atlassian/revoke").await;
    assert_eq!(status, StatusCode::OK);
    let a: RevokeResponse = serde_json::from_str(&body).unwrap();
    assert!(a.disconnected && !a.at_service);
    assert!(a.hint.unwrap().contains("id.atlassian.com"));
    assert!(!p.secrets.has(secrets::ATLASSIAN_OAUTH_TOKENS).unwrap());
    assert_eq!(p.state.atlassian_site(), None);
    // Revoking without sign-in is no error.
    assert!(
        p.state
            .oauth_revoke(Service::Atlassian)
            .await
            .unwrap()
            .disconnected
    );
}

fn expired(p: &TestBench, d: Service, refresh: Option<&str>) {
    let t = Tokens {
        access: Secret::new("alt_zugriff"),
        refresh: refresh.map(Secret::new),
        expires_in_s: Some(1),
    };
    p.state.oauth_tokens_set(d, &t).unwrap();
}

#[tokio::test]
async fn expired_tokens_are_refreshed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "neu_zugriff", "expires_in": 28800, "refresh_token": "neu_erneuern"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_refresh().await;
    let t = p.state.oauth_tokens(Service::GitHub).unwrap();
    assert_eq!(t.access.expose(), "neu_zugriff");
    assert_eq!(t.refresh.unwrap().expose(), "neu_erneuern");
    // Still valid: no second call (`expect(1)`).
    p.state.oauth_refresh().await;
    // The client gets the new token.
    let d = p.state.services_build();
    assert!(d.github.is_some());
}

#[tokio::test]
async fn fails_the_refresh_is_the_service_disconnected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_refresh_token", "error_description": "abgelaufen"
        })))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_refresh().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_none());
    let s = p.state.service_status(Service::GitHub, false);
    assert!(!s.connected);
    assert_eq!(s.kind, AuthKind::None);

    // Without a refresh token the expired access cannot be saved.
    expired(&p, Service::GitHub, None);
    p.state.oauth_refresh().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_none());
}

#[tokio::test]
async fn outage_the_service_disconnects_not() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_refresh().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_some());
}

#[tokio::test]
async fn oauth_goes_before_the_api_token() {
    let server = MockServer::start().await;
    let p = snapshot(&server, None, true);
    p.secrets
        .set(secrets::GITHUB, &Secret::new("ghp_api_token_123"))
        .unwrap();
    let status = p.state.service_status(Service::GitHub, true);
    assert_eq!((status.connected, status.kind), (true, AuthKind::Token));
    p.state
        .oauth_tokens_set(
            Service::GitHub,
            &Tokens {
                access: Secret::new("ghu_x"),
                refresh: None,
                expires_in_s: None,
            },
        )
        .unwrap();
    let status = p.state.service_status(Service::GitHub, true);
    assert_eq!(status.kind, AuthKind::OAuth);
    assert!(status.oauth_possible);
    // After disconnecting the token applies again.
    p.state.oauth_revoke(Service::GitHub).await.unwrap();
    assert_eq!(
        p.state.service_status(Service::GitHub, true).kind,
        AuthKind::Token
    );
}

#[tokio::test]
async fn callback_is_a_navigation_but_only_from_this_machine() {
    let server = MockServer::start().await;
    let p = snapshot(&server, None, true);
    // Navigation of the browser: GET without JSON content type and without origin.
    let req = Request::builder()
        .uri("/api/oauth/github/callback?code=a&state=b")
        .header("host", "localhost:8787")
        .header("sec-fetch-mode", "navigate")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
        .body(Body::empty())
        .unwrap();
    let r = p.app.clone().oneshot(req).await.unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    // Foreign host (DNS rebinding) and foreign peer stay out.
    for (host, peer) in [
        ("evil.example:8787", [127, 0, 0, 1]),
        ("127.0.0.1:8787", [10, 0, 0, 5]),
    ] {
        let req = Request::builder()
            .uri("/api/oauth/github/callback?code=a&state=b")
            .header("host", host)
            .extension(ConnectInfo(SocketAddr::from((peer, 50000))))
            .body(Body::empty())
            .unwrap();
        let r = p.app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{host}");
    }
}

#[test]
fn addresses_and_encoding() {
    assert_eq!(
        redirect_uri(Service::GitHub, 8787),
        "http://127.0.0.1:8787/api/oauth/github/callback"
    );
    assert_eq!(
        redirect_uri(Service::Atlassian, 8787),
        "http://localhost:8787/api/oauth/atlassian/callback"
    );
    assert_eq!(encoded("Ä b&c=d#"), "%C3%84%20b%26c%3Dd%23");
}
