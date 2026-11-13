//! Follow-up inputs (read) and creating a pull request (write) against a mock server.
//! None of this has been checked against the real service.

use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::{Error, Secret};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "ghp_sehr_geheim_123";

fn client(server: &MockServer) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new(TOKEN)).unwrap()
}

fn bearer() -> impl wiremock::Match {
    header("authorization", format!("Bearer {TOKEN}").as_str())
}

#[tokio::test]
async fn review_input_groups_threads_and_keeps_review_texts() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls/7/comments"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "user": {"login": "mira"}, "body": "Warum unwrap?", "path": "a.rs", "line": 4},
            {"id": 2, "in_reply_to_id": 1, "user": {"login": "ich"}, "body": "Stimmt.", "path": "a.rs", "line": 4},
        ])))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls/7/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"user": {"login": "mira"}, "state": "CHANGES_REQUESTED", "body": "Bitte Tests.", "submitted_at": "2026-01-01T10:00:00Z"},
            {"user": {"login": "tom"}, "state": "APPROVED", "body": "ok", "submitted_at": "2026-01-01T11:00:00Z"},
        ])))
        .expect(1)
        .mount(&s)
        .await;
    let input = client(&s).review_input("a/x", 7).await.unwrap();
    assert_eq!(input.threads.len(), 1);
    assert_eq!(input.threads[0].comments.len(), 2);
    assert_eq!(input.notes.len(), 1);
    assert!(!input.is_empty());
}

#[tokio::test]
async fn failed_checks_carry_output_and_annotations_only_for_red_runs() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 7, "title": "t", "user": {"login": "ich"},
            "head": {"ref": "claude/BAU-1-x", "sha": "abc123"},
            "base": {"ref": "main"}
        })))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/commits/abc123/check-runs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 11, "name": "Lint", "status": "completed", "conclusion": "failure",
                 "output": {"title": "1 Fehler", "summary": "Lint rot", "text": null}},
                {"id": 12, "name": "Build", "status": "completed", "conclusion": "success"},
            ]})),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/check-runs/11/annotations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"path": "src/a.rs", "start_line": 9, "annotation_level": "failure", "message": "unused"}
        ])))
        .expect(1)
        .mount(&s)
        .await;
    let checks = client(&s).failed_checks("a/x", 7).await.unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "Lint");
    assert!(checks[0].excerpt.contains("Lint rot"));
    assert!(checks[0].excerpt.contains("src/a.rs:9: unused"));
}

#[tokio::test]
async fn missing_annotations_do_not_hide_the_failed_check() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 7, "user": {"login": "ich"},
            "head": {"ref": "claude/x", "sha": "abc"}, "base": {"ref": "main"}
        })))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/commits/abc/check-runs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 5, "name": "Tests", "status": "completed", "conclusion": "timed_out"}
            ]})),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/check-runs/5/annotations"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&s)
        .await;
    let checks = client(&s).failed_checks("a/x", 7).await.unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].excerpt, "");
}

#[tokio::test]
async fn existing_pull_request_for_the_branch_is_found() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls"))
        .and(query_param("head", "a:claude/BAU-1-x"))
        .and(query_param("state", "open"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"number": 12, "html_url": "https://github.example/a/x/pull/12", "state": "open",
             "head": {"ref": "claude/BAU-1-x", "sha": "abc"}}
        ])))
        .expect(1)
        .mount(&s)
        .await;
    let found = client(&s)
        .pull_for_branch("a/x", "claude/BAU-1-x")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.number, 12);
    assert!(found.html_url.ends_with("/pull/12"));

    let empty = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&empty)
        .await;
    assert!(
        client(&empty)
            .pull_for_branch("a/x", "claude/BAU-1-x")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn pull_request_is_created_with_exactly_the_approved_fields() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/pulls"))
        .and(bearer())
        .and(body_json(json!({
            "title": "BAU-1: Ring", "head": "claude/BAU-1-ring", "base": "main",
            "body": "Text", "draft": true, "maintainer_can_modify": false
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 31, "html_url": "https://github.example/a/x/pull/31", "state": "open"
        })))
        .expect(1)
        .mount(&s)
        .await;
    let p = client(&s)
        .pull_create(
            "a/x",
            "claude/BAU-1-ring",
            "main",
            "BAU-1: Ring",
            "Text",
            true,
        )
        .await
        .unwrap();
    assert_eq!(p.number, 31);
}

#[tokio::test]
async fn pull_request_errors_are_told_apart() {
    for (status, body, check) in [
        (401, json!({"message": "Bad credentials"}), "401"),
        (403, json!({"message": "Resource not accessible"}), "403"),
        (
            422,
            json!({"message": "Validation Failed", "errors": [{"message": "No commits between main and claude/x"}]}),
            "422",
        ),
    ] {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/a/x/pulls"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&s)
            .await;
        let e = client(&s)
            .pull_create("a/x", "claude/x", "main", "t", "b", false)
            .await
            .unwrap_err();
        match (check, &e) {
            ("401", Error::NotSignedIn) | ("403", Error::NoPermission) => {}
            ("422", Error::Rejected(m)) => assert!(m.contains("No commits"), "{m}"),
            _ => panic!("{check}: {e:?}"),
        }
        assert!(!e.to_string().contains(TOKEN));
    }
}
