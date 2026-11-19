//! A fine-grained token cannot read check runs (403). The pull requests still load, with the
//! combined commit status or with unknown checks. Mock server only; GitHub's real 403 is not checked.

use super::*;
use cockpit_core::Checks;

fn json_body(v: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(v)
}

fn forbidden() -> ResponseTemplate {
    ResponseTemplate::new(403).set_body_json(json!({
        "message": "Resource not accessible by personal access token"
    }))
}

async fn deny_check_runs(s: &MockServer) {
    for sha in ["aaa111".to_string(), sha91()] {
        get(
            s,
            &format!("/repos/{REPO}/commits/{sha}/check-runs"),
            forbidden(),
        )
        .await;
    }
}

#[tokio::test]
async fn pull_requests_load_with_the_commit_status_when_check_runs_are_denied() {
    let g = github_server_without_checks().await;
    deny_check_runs(&g).await;
    get(
        &g,
        &format!("/repos/{REPO}/commits/aaa111/status"),
        json_body(json!({"state": "success", "total_count": 1, "statuses": []})),
    )
    .await;
    get(
        &g,
        &format!("/repos/{REPO}/commits/{}/status", sha91()),
        json_body(json!({"state": "failure", "total_count": 2, "statuses": []})),
    )
    .await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    let pr87 = ws
        .item("gh-acme-baukasten-87")
        .expect("PR 87 is not skipped");
    assert_eq!(pr87.pr.as_ref().unwrap().checks, Checks::Passed);
    let pr91 = ws
        .item("gh-acme-baukasten-91")
        .expect("PR 91 is not skipped");
    assert_eq!(pr91.pr.as_ref().unwrap().checks, Checks::Failed);
    assert!(ws.review(87).unwrap().check_runs.is_empty());
    assert!(ws.sync.state_of(Source::GitHub).unwrap().is_ok());
}

#[tokio::test]
async fn checks_are_unknown_when_the_status_is_denied_too_and_the_pull_request_loads() {
    let g = github_server_without_checks().await;
    deny_check_runs(&g).await;
    // No answer for `/status`: wiremock replies 404, which also counts as "not readable".
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    let pr87 = ws
        .item("gh-acme-baukasten-87")
        .expect("PR 87 is not skipped");
    assert_eq!(pr87.pr.as_ref().unwrap().checks, Checks::Unknown);
    assert!(ws.item("gh-acme-baukasten-91").is_some());
    assert_eq!(ws.review(87).unwrap().checks, Checks::Unknown);
    assert!(ws.sync.state_of(Source::GitHub).unwrap().is_ok());
}

#[tokio::test]
async fn check_runs_are_not_asked_again_after_a_denial() {
    let g = github_server_without_checks().await;
    // One request for the whole repo, not one per pull request and not one per sync round.
    Mock::given(method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/repos/.*/commits/.*/check-runs$",
        ))
        .respond_with(forbidden())
        .expect(1)
        .mount(&g)
        .await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    sync(&d, &ws, NOW_MS + 60_000).await;
    sync(&d, &ws, NOW_MS + 120_000).await;
    // `expect(1)` is verified when the server drops.
}
