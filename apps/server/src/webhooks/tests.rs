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

const GH_SECRET: &str = "dummy-github-webhook-secret";
const JIRA_SECRET: &str = "dummy-jira-webhook-secret";

// ---- signature ---------------------------------------------------------------------------

/// RFC 4231, test cases 1, 2 and 6 (key longer than the block size).
#[test]
fn hmac_matches_the_rfc_vectors() {
    assert_eq!(
        hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hex(&hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn signature_valid_cases() {
    let body = br#"{"a":1}"#;
    let good = signature_header(b"secret", body);
    assert!(signature_valid(b"secret", body, Some(&good)));
    // Hex in upper case is the same number.
    let upper = format!(
        "sha256={}",
        good.trim_start_matches("sha256=").to_uppercase()
    );
    assert!(signature_valid(b"secret", body, Some(&upper)));
    // Wrong secret, other body, missing header.
    assert!(!signature_valid(b"other", body, Some(&good)));
    assert!(!signature_valid(b"secret", br#"{"a":2}"#, Some(&good)));
    assert!(!signature_valid(b"secret", body, None));
    // Wrong prefix, wrong length, not hex, empty.
    assert!(!signature_valid(
        b"secret",
        body,
        Some(good.trim_start_matches("sha256="))
    ));
    assert!(!signature_valid(b"secret", body, Some("sha1=abcd")));
    assert!(!signature_valid(
        b"secret",
        body,
        Some(&good[..good.len() - 2])
    ));
    assert!(!signature_valid(
        b"secret",
        body,
        Some(&format!("{good}00"))
    ));
    assert!(!signature_valid(
        b"secret",
        body,
        Some(&format!("sha256={}", "zz".repeat(32)))
    ));
    assert!(!signature_valid(b"secret", body, Some("sha256=")));
    assert!(!signature_valid(b"secret", body, Some("")));
}

#[test]
fn constant_time_compare() {
    assert!(equal_constant_time(b"abc", b"abc"));
    assert!(!equal_constant_time(b"abc", b"abd"));
    assert!(!equal_constant_time(b"abc", b"abcd"));
}

#[test]
fn generated_secrets_are_long_and_different() {
    let a = secret_generate().unwrap();
    let b = secret_generate().unwrap();
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

// ---- debounce, interval --------------------------------------------------------------------

#[test]
fn gate_merges_triggers() {
    let t0 = Instant::now();
    let d = Duration::from_secs(5);
    let mut g = Gate::default();
    // The first one runs at once; the next ones merge until it has started.
    assert_eq!(g.admit(t0, d), Admit::After(Duration::ZERO));
    assert_eq!(g.admit(t0, d), Admit::Merged);
    g.started(t0);
    // Within the debounce time: one run after the rest of it, further ones merge.
    let t1 = t0 + Duration::from_secs(2);
    assert_eq!(g.admit(t1, d), Admit::After(Duration::from_secs(3)));
    assert_eq!(g.admit(t1, d), Admit::Merged);
    g.started(t0 + Duration::from_secs(5));
    // Long afterwards: at once again.
    let t2 = t0 + Duration::from_secs(60);
    assert_eq!(g.admit(t2, d), Admit::After(Duration::ZERO));
}

#[test]
fn rate_limit_and_replay_window() {
    let t0 = Instant::now();
    let mut p = PerSource::default();
    for _ in 0..RATE_LIMIT {
        assert!(p.rate_ok(t0));
    }
    assert!(!p.rate_ok(t0));
    assert!(p.rate_ok(t0 + RATE_WINDOW + Duration::from_secs(1)));

    let keys = vec!["id:1".to_string()];
    assert!(!p.seen_before(t0, &keys));
    assert!(p.seen_before(t0 + Duration::from_secs(5), &keys));
    assert!(!p.seen_before(t0 + REPLAY_TTL + Duration::from_secs(1), &keys));
    // The memory stays bounded.
    for i in 0..(REPLAY_MAX * 2) {
        p.seen_before(t0, &[format!("id:x{i}")]);
    }
    assert!(p.seen.len() <= REPLAY_MAX);
}

#[test]
fn poll_interval_follows_the_webhooks() {
    let base = Duration::from_secs(120);
    let fallback = Duration::from_secs(600);
    let s = Duration::from_secs;
    // Webhooks off, or none arrived yet: every tick polls.
    assert!(poll_due(false, Some(s(1)), Some(s(1)), base, fallback));
    assert!(poll_due(true, None, Some(s(1)), base, fallback));
    // Arriving regularly: only the safety net polls ...
    assert!(!poll_due(true, Some(s(100)), Some(s(120)), base, fallback));
    assert!(!poll_due(true, Some(s(100)), Some(s(480)), base, fallback));
    // ... every `fallback` (with half an interval of slack for the tick).
    assert!(poll_due(true, Some(s(100)), Some(s(540)), base, fallback));
    assert!(poll_due(true, Some(s(100)), None, base, fallback));
    // The last webhook is older than two intervals: back to the normal interval.
    assert!(poll_due(true, Some(s(241)), Some(s(120)), base, fallback));
    assert!(!poll_due(true, Some(s(240)), Some(s(120)), base, fallback));
}

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

#[tokio::test]
async fn a_signed_event_triggers_a_sync_of_that_source_only() {
    let f = plain().await;
    assert_eq!(
        gh(&f, "pull_request", "d1", r#"{"action":"opened"}"#).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count(&f.github, "/user").await, 1);
    assert_eq!(
        count(&f.jira, "/rest/api/3/myself").await,
        0,
        "Jira must not be asked"
    );

    assert_eq!(
        jira(
            &f,
            r#"{"webhookEvent":"jira:issue_updated","issue":{"key":"WEB-1"}}"#
        )
        .await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.jira, "/rest/api/3/myself", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count(&f.github, "/user").await, 1);
    assert_eq!(count(&f.jira, "/rest/api/3/myself").await, 1);
}

#[tokio::test]
async fn the_payload_is_never_taken_over_as_data() {
    let f = plain().await;
    let body = r#"{"action":"opened","pull_request":{"title":"INJECTED-TITLE"}}"#;
    assert_eq!(
        gh(&f, "pull_request", "d1", body).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, snapshot) = call(
        &f.state,
        "GET",
        "/api/workspace",
        "127.0.0.1:8787",
        &vec![],
        vec![],
    )
    .await;
    assert!(!snapshot.contains("INJECTED-TITLE"));
}

#[tokio::test]
async fn rejects_bad_signatures_without_syncing() {
    let f = plain().await;
    let body = br#"{"action":"opened"}"#.to_vec();
    let good = gh_headers(&body, "pull_request", "d1");
    let post = |h: Headers, b: Vec<u8>| {
        let s = f.state.clone();
        async move { call(&s, "POST", GITHUB_PATH, "cockpit.example", &h, b).await }
    };
    // Missing.
    let mut h = good.clone();
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    assert_eq!(post(h, body.clone()).await.0, StatusCode::UNAUTHORIZED);
    // Wrong secret.
    let mut h = good.clone();
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    h.push(("x-hub-signature-256", signature_header(b"wrong", &body)));
    assert_eq!(post(h, body.clone()).await.0, StatusCode::UNAUTHORIZED);
    // Right signature, other body.
    let (status, text) = post(good.clone(), br#"{"action":"closed"}"#.to_vec()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(text.is_empty(), "no details to the outside");
    // Wrong length, not hex.
    for bad in ["sha256=abcd", "sha256=zz", "nonsense"] {
        let mut h = good.clone();
        h.retain(|(k, _)| *k != "x-hub-signature-256");
        h.push(("x-hub-signature-256", bad.into()));
        assert_eq!(post(h, body.clone()).await.0, StatusCode::UNAUTHORIZED);
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
    assert!(
        f.state
            .inner
            .webhooks
            .last_received(Source::GitHub)
            .is_none()
    );
}

#[tokio::test]
async fn upper_case_hex_signature_is_accepted() {
    let f = plain().await;
    let body = br#"{"action":"opened"}"#.to_vec();
    let mut h = gh_headers(&body, "pull_request", "d1");
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    let upper = signature_header(GH_SECRET.as_bytes(), &body)
        .trim_start_matches("sha256=")
        .to_uppercase();
    h.push(("x-hub-signature-256", format!("sha256={upper}")));
    let (status, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn off_means_not_found_even_with_a_valid_signature() {
    let f = fixture(false, true, Duration::from_millis(50), 8787).await;
    assert_eq!(
        gh(&f, "pull_request", "d1", "{}").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"jira:issue_updated"}"#).await,
        StatusCode::NOT_FOUND
    );
    let (s, _) = call(
        &f.state,
        "GET",
        GITHUB_PATH,
        "cockpit.example",
        &vec![],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
}

#[tokio::test]
async fn without_a_secret_nothing_is_accepted() {
    let f = fixture(true, false, Duration::from_millis(50), 8787).await;
    assert_eq!(
        gh(&f, "pull_request", "d1", "{}").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"jira:issue_updated"}"#).await,
        StatusCode::NOT_FOUND
    );
    // An empty secret counts as missing; the signature of an empty key must not pass.
    f.secrets
        .set(crate::secrets::GITHUB_WEBHOOK, &Secret::new(""))
        .unwrap();
    let body = b"{}".to_vec();
    let mut h = gh_headers(&body, "pull_request", "d1");
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    h.push(("x-hub-signature-256", signature_header(b"", &body)));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_post_json_within_the_size_limit() {
    let f = plain().await;
    let (s, _) = call(
        &f.state,
        "GET",
        GITHUB_PATH,
        "cockpit.example",
        &vec![],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
    // Not JSON (form encoding is what GitHub offers as the other option).
    let body = b"payload=%7B%7D".to_vec();
    let mut h = gh_headers(&body, "pull_request", "d1");
    h.retain(|(k, _)| *k != "content-type");
    h.push(("content-type", "application/x-www-form-urlencoded".into()));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // `application/json; charset=utf-8` is fine.
    let body = b"{}".to_vec();
    let mut h = gh_headers(&body, "ping", "d9");
    h.retain(|(k, _)| *k != "content-type");
    h.push(("content-type", "Application/JSON; charset=utf-8".into()));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::OK);
    // Too big: with and without a declared length.
    let big = vec![b'a'; MAX_BODY + 1];
    let h = gh_headers(&big, "pull_request", "d2");
    let (s, _) = call(
        &f.state,
        "POST",
        GITHUB_PATH,
        "cockpit.example",
        &h,
        big.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let mut h = h;
    h.push(("content-length", "10".into()));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, big).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // Exactly at the limit still passes the size check (and fails on the signature).
    let edge = vec![b'a'; MAX_BODY];
    let mut h = gh_headers(b"x", "pull_request", "d3");
    h.retain(|(k, _)| *k != "content-length");
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, edge).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ping_is_answered_and_irrelevant_events_ignored() {
    let f = plain().await;
    assert_eq!(gh(&f, "ping", "p1", r#"{"zen":"x"}"#).await, StatusCode::OK);
    assert_eq!(
        gh(&f, "star", "s1", r#"{"a":1}"#).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"sprint_started"}"#).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(jira(&f, "not json").await, StatusCode::NO_CONTENT);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
    assert_eq!(count(&f.jira, "/rest/api/3/myself").await, 0);
    // They are still shown as "received" (event type only) ...
    let (_, event) = f
        .state
        .inner
        .webhooks
        .last_received(Source::GitHub)
        .unwrap();
    assert_eq!(event, "star");
    // ... but they do not count as regular webhooks for the poll interval.
    assert!(f.state.inner.webhooks.poll_due(
        Source::GitHub,
        Instant::now(),
        Duration::from_secs(120)
    ));
}

#[tokio::test]
async fn a_missing_github_event_header_is_a_bad_request() {
    let f = plain().await;
    let body = b"{}".to_vec();
    let mut h = gh_headers(&body, "x", "d");
    h.retain(|(k, _)| *k != "x-github-event");
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_replayed_delivery_is_ignored() {
    let f = plain().await;
    let body = r#"{"action":"opened"}"#;
    assert_eq!(
        gh(&f, "pull_request", "same-id", body).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    // The same ID again, and the same body under a new ID.
    assert_eq!(
        gh(&f, "pull_request", "same-id", r#"{"action":"closed"}"#).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        gh(&f, "pull_request", "other-id", body).await,
        StatusCode::ACCEPTED
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count(&f.github, "/user").await, 1);
    // A different delivery is a new one.
    assert_eq!(
        gh(&f, "pull_request", "third", r#"{"action":"edited"}"#).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 2).await;
}

#[tokio::test]
async fn bursts_are_merged_into_few_syncs() {
    let f = fixture(true, true, Duration::from_millis(400), 8787).await;
    assert_eq!(
        gh(&f, "pull_request", "b0", r#"{"n":0}"#).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    // While the debounce time runs: many webhooks, one more sync.
    for i in 1..=10 {
        let body = format!(r#"{{"n":{i}}}"#);
        assert_eq!(
            gh(&f, "check_run", &format!("b{i}"), &body).await,
            StatusCode::ACCEPTED
        );
    }
    wait_for(&f.github, "/user", 2).await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(count(&f.github, "/user").await, 2);
}

#[tokio::test]
async fn rate_limit_answers_429() {
    let f = plain().await;
    let mut last = StatusCode::OK;
    for _ in 0..=RATE_LIMIT {
        let (s, _) = call(
            &f.state,
            "POST",
            GITHUB_PATH,
            "cockpit.example",
            &vec![("content-type", "application/json".into())],
            b"{}".to_vec(),
        )
        .await;
        last = s;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    // The other source has its own budget.
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"cockpit:test"}"#).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn jira_accepts_the_signature_or_the_token() {
    let f = plain().await;
    let body = br#"{"webhookEvent":"jira:issue_created"}"#.to_vec();
    let json_type: Headers = vec![("content-type", "application/json".into())];
    // Token in the query.
    let uri = format!("{JIRA_PATH}?token={JIRA_SECRET}");
    let (s, _) = call(
        &f.state,
        "POST",
        &uri,
        "cockpit.example",
        &json_type,
        body.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    // Wrong or missing token.
    let body2 = br#"{"webhookEvent":"jira:issue_created","n":2}"#.to_vec();
    for uri in [
        format!("{JIRA_PATH}?token=nope"),
        format!("{JIRA_PATH}?token="),
        JIRA_PATH.to_string(),
    ] {
        let (s, _) = call(
            &f.state,
            "POST",
            &uri,
            "cockpit.example",
            &json_type,
            body2.clone(),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri}");
    }
    // A wrong signature is not rescued by a right token.
    let mut h = json_type.clone();
    h.push(("x-hub-signature", signature_header(b"wrong", &body2)));
    let uri = format!("{JIRA_PATH}?token={JIRA_SECRET}");
    let (s, _) = call(&f.state, "POST", &uri, "cockpit.example", &h, body2).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // GitHub never takes the token.
    let uri = format!("{GITHUB_PATH}?token={GH_SECRET}");
    let (s, _) = call(
        &f.state,
        "POST",
        &uri,
        "cockpit.example",
        &gh_headers(b"x", "ping", "z")
            .into_iter()
            .filter(|(k, _)| *k != "x-hub-signature-256")
            .collect(),
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

// ---- the host exception is only these two routes ------------------------------------------

#[tokio::test]
async fn a_foreign_host_only_reaches_the_two_webhook_routes() {
    let f = plain().await;
    let json_type: Headers = vec![("content-type", "application/json".into())];
    let routes = [
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
        ("POST", "/api/actions"),
        ("POST", "/api/sync"),
        ("GET", "/api/events"),
        ("GET", "/api/setup"),
        ("PUT", "/api/config"),
        ("PUT", "/api/secrets/github"),
        ("DELETE", "/api/secrets/github"),
        ("POST", "/api/setup/webhook-secret/github"),
        ("POST", "/api/setup/webhook-test/github"),
        ("GET", "/api/oauth/github/start"),
        ("GET", "/api/agent/models"),
        ("GET", "/api/webhooks/other"),
        ("GET", "/api/webhooks"),
        ("POST", "/api/webhooks/github/extra"),
        ("GET", "/api/gibt-es-nicht"),
    ];
    for host in [
        "cockpit.example",
        "evil.example:8787",
        "127.0.0.1.evil.example",
        "127.0.0.1",
    ] {
        for (m, p) in routes {
            let (s, _) = call(&f.state, m, p, host, &json_type, b"{}".to_vec()).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{m} {p} with host {host}");
        }
    }
    // The same routes work with the real host (control).
    let (s, _) = call(
        &f.state,
        "GET",
        "/api/workspace",
        "127.0.0.1:8787",
        &vec![],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // The webhook routes work with a foreign host.
    assert_eq!(gh(&f, "ping", "x1", "{}").await, StatusCode::OK);
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"cockpit:test"}"#).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_remote_peer_is_refused_on_the_webhook_routes() {
    let f = plain().await;
    let body = b"{}".to_vec();
    let h = gh_headers(&body, "ping", "r1");
    let mut b = Request::builder()
        .method("POST")
        .uri(GITHUB_PATH)
        .header("host", "cockpit.example")
        .extension(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
    for (k, v) in &h {
        b = b.header(*k, v);
    }
    let app = router(f.state.clone(), PathBuf::from("/x"));
    let answer = app
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    assert_eq!(answer.status(), StatusCode::FORBIDDEN);
}

// ---- assistant: secret, status, test ------------------------------------------------------

#[tokio::test]
async fn the_generated_secret_is_shown_once_and_never_again() {
    let f = fixture(true, false, Duration::from_millis(50), 8787).await;
    let h: Headers = vec![("content-type", "application/json".into())];
    let (s, first) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/github",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let secret = serde_json::from_str::<serde_json::Value>(&first).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(secret.len(), 64);
    // It is stored (and the signature check uses it) ...
    assert_eq!(
        f.secrets
            .get(crate::secrets::GITHUB_WEBHOOK)
            .unwrap()
            .unwrap()
            .expose(),
        secret
    );
    // ... but no other answer contains it.
    for (m, p) in [
        ("GET", "/api/setup"),
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
    ] {
        let (_, text) = call(&f.state, m, p, "127.0.0.1:8787", &h, vec![]).await;
        assert!(!text.contains(&secret), "{p}");
    }
    let (_, setup) = call(&f.state, "GET", "/api/setup", "127.0.0.1:8787", &h, vec![]).await;
    let setup: serde_json::Value = serde_json::from_str(&setup).unwrap();
    assert_eq!(setup["webhooks"]["github"]["secret_set"], true);
    assert_eq!(setup["webhooks"]["jira"]["secret_set"], false);
    assert_eq!(setup["webhooks"]["github"]["path"], "/api/webhooks/github");
    // Renewing gives a different one and replaces the old.
    let (_, second) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/github",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    assert_ne!(first, second);
    assert!(
        !f.secrets
            .get(crate::secrets::GITHUB_WEBHOOK)
            .unwrap()
            .unwrap()
            .expose()
            .eq(&secret)
    );
    // An unknown source, and a foreign origin, are refused.
    let (s, _) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/gitlab",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let mut evil = h.clone();
    evil.push(("origin", "https://evil.example".into()));
    let (s, _) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/github",
        "127.0.0.1:8787",
        &evil,
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

/// Starts the real router on a free port so that the local test request has somewhere to go.
async fn serve(f: &Fixture) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(f.state.clone(), PathBuf::from("/x"));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    port
}

#[tokio::test]
async fn the_test_button_makes_a_signed_round_trip_over_loopback() {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let f = fixture(true, true, Duration::from_millis(50), port).await;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let app = router(f.state.clone(), PathBuf::from("/x"));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    let h: Headers = vec![("content-type", "application/json".into())];
    for source in ["github", "jira"] {
        let (s, text) = call(
            &f.state,
            "POST",
            &format!("/api/setup/webhook-test/{source}"),
            &format!("127.0.0.1:{port}"),
            &h,
            vec![],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["ok"], true, "{source}: {text}");
        assert!(!text.contains(GH_SECRET) && !text.contains(JIRA_SECRET));
        // A second test is not swallowed by the replay check.
        let (_, text) = call(
            &f.state,
            "POST",
            &format!("/api/setup/webhook-test/{source}"),
            &format!("127.0.0.1:{port}"),
            &h,
            vec![],
        )
        .await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap()["ok"],
            true
        );
    }
    let (_, event) = f
        .state
        .inner
        .webhooks
        .last_received(Source::GitHub)
        .unwrap();
    assert_eq!(event, TEST_EVENT);
    // A test does not trigger a sync and does not make the webhooks "regular".
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
    assert!(f.state.inner.webhooks.poll_due(
        Source::Jira,
        Instant::now(),
        Duration::from_secs(120)
    ));

    // A wrong secret in the store would be reported: simulate by changing it between the calls?
    // No: the test signs with the stored one. Without a secret it says so.
    f.secrets.delete(crate::secrets::GITHUB_WEBHOOK).unwrap();
    let (_, text) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-test/github",
        &format!("127.0.0.1:{port}"),
        &h,
        vec![],
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["ok"], false);
    assert!(v["message"].as_str().unwrap().contains("Secret"));
}

#[tokio::test]
async fn the_test_button_explains_when_webhooks_are_off() {
    let f = fixture(false, true, Duration::from_millis(50), 8787).await;
    let _ = serve(&f).await;
    let h: Headers = vec![("content-type", "application/json".into())];
    let (_, text) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-test/github",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["ok"], false);
    assert!(v["message"].as_str().unwrap().contains("aktiviert"));
}

#[test]
fn query_token_and_event_names() {
    assert_eq!(query_token(Some("a=1&token=xyz")), Some("xyz"));
    assert_eq!(query_token(Some("token=")), None);
    assert_eq!(query_token(None), None);
    assert!(event_name_ok("jira:issue_updated"));
    assert!(!event_name_ok(""));
    assert!(!event_name_ok("a b"));
    assert!(!event_name_ok(&"a".repeat(65)));
    assert!(!event_name_ok("<script>"));
}
