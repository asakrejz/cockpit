//! Checks with a fine-grained token: no "Checks" permission, so the check runs answer 403.

use cockpit_core::Checks;
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::{Error, Secret};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer, token: &str) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new(token)).unwrap()
}

fn denied() -> ResponseTemplate {
    ResponseTemplate::new(403).set_body_json(json!({
        "message": "Resource not accessible by personal access token"
    }))
}

async fn mount(s: &MockServer, p: &str, r: ResponseTemplate, times: u64) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(r)
        .expect(times)
        .mount(s)
        .await;
}

async fn pull_mock(s: &MockServer) {
    mount(
        s,
        "/repos/a/x/pulls/7",
        ResponseTemplate::new(200).set_body_json(json!({
            "number": 7, "title": "t", "user": {"login": "ich"},
            "head": {"ref": "claude/BAU-1-x", "sha": "abc123"}, "base": {"ref": "main"}
        })),
        1,
    )
    .await;
}

#[tokio::test]
async fn check_runs_win_when_readable() {
    let s = MockServer::start().await;
    mount(
        &s,
        "/repos/a/x/commits/abc123/check-runs",
        ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
            {"id": 1, "name": "Build", "status": "completed", "conclusion": "failure"}
        ]})),
        1,
    )
    .await;
    // The status is not even asked.
    mount(&s, "/repos/a/x/commits/abc123/status", denied(), 0).await;
    let loaded = client(&s, "dummy-token-ok")
        .checks_load("a/x", "abc123")
        .await
        .unwrap();
    assert_eq!(loaded.checks, Checks::Failed);
    assert_eq!(loaded.runs.len(), 1);
}

#[tokio::test]
async fn a_403_on_check_runs_falls_back_to_the_commit_status() {
    let s = MockServer::start().await;
    mount(&s, "/repos/a/x/commits/abc123/check-runs", denied(), 1).await;
    mount(
        &s,
        "/repos/a/x/commits/abc123/status",
        ResponseTemplate::new(200)
            .set_body_json(json!({"state": "success", "total_count": 1, "statuses": []})),
        2,
    )
    .await;
    let c = client(&s, "dummy-token-fallback");
    let first = c.checks_load("a/x", "abc123").await.unwrap();
    assert_eq!(first.checks, Checks::Passed);
    assert!(first.runs.is_empty());
    // The second call knows about the denial: no new check-runs request (`expect(1)` above).
    assert_eq!(
        c.checks_load("a/x", "abc123").await.unwrap().checks,
        Checks::Passed
    );
}

#[tokio::test]
async fn a_404_on_check_runs_falls_back_too() {
    let s = MockServer::start().await;
    mount(
        &s,
        "/repos/a/x/commits/abc123/check-runs",
        ResponseTemplate::new(404),
        1,
    )
    .await;
    mount(
        &s,
        "/repos/a/x/commits/abc123/status",
        ResponseTemplate::new(200)
            .set_body_json(json!({"state": "pending", "total_count": 1, "statuses": []})),
        1,
    )
    .await;
    let loaded = client(&s, "dummy-token-404")
        .checks_load("a/x", "abc123")
        .await
        .unwrap();
    assert_eq!(loaded.checks, Checks::Running);
}

#[tokio::test]
async fn checks_are_unknown_when_both_are_denied() {
    let s = MockServer::start().await;
    mount(&s, "/repos/a/x/commits/abc123/check-runs", denied(), 1).await;
    mount(&s, "/repos/a/x/commits/abc123/status", denied(), 1).await;
    let loaded = client(&s, "dummy-token-both")
        .checks_load("a/x", "abc123")
        .await
        .unwrap();
    assert_eq!(loaded.checks, Checks::Unknown);
    assert!(loaded.runs.is_empty());
}

#[tokio::test]
async fn a_status_without_entries_is_unknown() {
    let s = MockServer::start().await;
    mount(&s, "/repos/a/x/commits/abc123/check-runs", denied(), 1).await;
    mount(
        &s,
        "/repos/a/x/commits/abc123/status",
        ResponseTemplate::new(200)
            .set_body_json(json!({"state": "pending", "total_count": 0, "statuses": []})),
        1,
    )
    .await;
    let loaded = client(&s, "dummy-token-empty")
        .checks_load("a/x", "abc123")
        .await
        .unwrap();
    assert_eq!(loaded.checks, Checks::Unknown);
}

#[tokio::test]
async fn other_errors_are_not_swallowed() {
    let s = MockServer::start().await;
    mount(
        &s,
        "/repos/a/x/commits/abc123/check-runs",
        ResponseTemplate::new(500),
        1,
    )
    .await;
    let e = client(&s, "dummy-token-500")
        .checks_load("a/x", "abc123")
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Status(500)), "{e:?}");
}

#[tokio::test]
async fn another_token_asks_again() {
    let s = MockServer::start().await;
    // Two different tokens (a changed sign-in method): each asks once.
    mount(&s, "/repos/a/x/commits/abc123/check-runs", denied(), 2).await;
    mount(&s, "/repos/a/x/commits/abc123/status", denied(), 2).await;
    for token in ["dummy-token-one", "dummy-token-two"] {
        client(&s, token)
            .checks_load("a/x", "abc123")
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn failed_checks_say_plainly_that_checks_are_not_readable() {
    let s = MockServer::start().await;
    pull_mock(&s).await;
    mount(&s, "/repos/a/x/commits/abc123/check-runs", denied(), 1).await;
    let e = client(&s, "dummy-token-logs")
        .failed_checks("a/x", 7)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Coded(_)), "{e:?}");
    assert!(e.to_string().contains("Checks"), "{e}");
}

#[tokio::test]
async fn re_requesting_a_check_says_plainly_that_checks_are_not_readable() {
    let s = MockServer::start().await;
    pull_mock(&s).await;
    mount(&s, "/repos/a/x/commits/abc123/check-runs", denied(), 1).await;
    let e = client(&s, "dummy-token-rerun")
        .check_again_start("a/x", 7, "Build")
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Coded(_)), "{e:?}");
}
