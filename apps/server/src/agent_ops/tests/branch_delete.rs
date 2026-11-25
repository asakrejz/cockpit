//! Marking runs as done and deleting the branch of a run: a mock server as GitHub, no real
//! service. Every refusal also checks that no `DELETE` reached GitHub.

use super::*;
use crate::config::GitHubConfig;
use cockpit_core::{AgentRun, Closure, PullRequestRef, RunState};
use cockpit_i18n::Lang;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TITLE: &str = "Toggle: Fokus-Ring fehlt im Dark Mode";
const REF_PATH: &str = "/repos/firma/app/git/refs/heads/claude/BAU-142-toggle-fokus-ring-fehlt";

fn finished_run() -> AgentRun {
    let mut run = AgentRun::new("BAU-142", TITLE);
    run.state = RunState::Done;
    run.repo = "firma/app".into();
    run.pull_request = Some(PullRequestRef {
        number: 31,
        url: String::new(),
        existed: false,
    });
    run
}

/// A world with one finished run whose pull request is #31, GitHub being `github`.
fn world_with_run(github: &MockServer, run: AgentRun) -> World {
    let w = world(true);
    let mut ws = demo::workspace();
    ws.runs = vec![run];
    w.state.inner.live.set_state_for_test(ws);
    let sources = crate::sync::Sources {
        github: Some(GitHubConfig {
            api_url: github.uri(),
            repos: vec!["firma/app".into()],
        }),
        ..crate::sync::Sources::default()
    };
    *w.state.inner.sources.write().unwrap() = Arc::new(sources);
    w
}

async fn pull(github: &MockServer, state: &str, merged: bool, head: &str) {
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/31"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 31, "title": "t", "user": {"login": "ich"},
            "head": {"ref": head, "sha": "a1"}, "base": {"ref": "main", "sha": "b2"},
            "state": state, "merged": merged
        })))
        .mount(github)
        .await;
}

async fn repo_and_branch(github: &MockServer, default: &str, protected: bool) {
    Mock::given(method("GET"))
        .and(path("/repos/firma/app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": default})))
        .mount(github)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/repos/firma/app/branches/claude/BAU-142-toggle-fokus-ring-fehlt",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"protected": protected})))
        .mount(github)
        .await;
}

async fn deletes(github: &MockServer) -> Vec<String> {
    github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "DELETE")
        .map(|r| r.url.path().to_string())
        .collect()
}

async fn allow_delete(github: &MockServer) {
    Mock::given(method("DELETE"))
        .and(path(REF_PATH))
        .respond_with(ResponseTemplate::new(204))
        .mount(github)
        .await;
}

/// Everything refused: check says no with a reason, delete fails, GitHub saw no DELETE.
async fn assert_refused(w: &World, github: &MockServer, part: &str) {
    let check = w.state.branch_check("BAU-142", "").await;
    assert!(!check.allowed, "{check:?}");
    let reason = check.reason.unwrap_or_default();
    assert!(reason.render(Lang::De).contains(part), "{reason:?}");
    assert!(w.state.branch_delete("BAU-142", "").await.is_err());
    assert_eq!(deletes(github).await, Vec::<String>::new());
}

#[tokio::test]
async fn a_merged_pull_request_allows_deleting_exactly_its_branch() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, BRANCH).await;
    repo_and_branch(&github, "main", false).await;
    allow_delete(&github).await;
    let w = world_with_run(&github, finished_run());
    assert_eq!(w.run().branch(), BRANCH);
    let check = w.state.branch_check("BAU-142", "").await;
    assert!(check.allowed, "{check:?}");
    assert_eq!(check.branch, BRANCH);
    assert_eq!(deletes(&github).await.len(), 0, "checking deletes nothing");
    let done = w.state.branch_delete("BAU-142", "").await.unwrap();
    assert_eq!(done.branch, BRANCH);
    assert_eq!(deletes(&github).await, vec![REF_PATH.to_string()]);
}

#[tokio::test]
async fn a_closed_pull_request_works_too() {
    let github = MockServer::start().await;
    pull(&github, "closed", false, BRANCH).await;
    repo_and_branch(&github, "main", false).await;
    allow_delete(&github).await;
    let w = world_with_run(&github, finished_run());
    assert!(w.state.branch_delete("BAU-142", "").await.is_ok());
}

#[tokio::test]
async fn an_open_pull_request_keeps_its_branch() {
    let github = MockServer::start().await;
    pull(&github, "open", false, BRANCH).await;
    repo_and_branch(&github, "main", false).await;
    allow_delete(&github).await;
    let w = world_with_run(&github, finished_run());
    assert_refused(&w, &github, "noch offen").await;
}

#[tokio::test]
async fn a_protected_branch_stays() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, BRANCH).await;
    repo_and_branch(&github, "main", true).await;
    allow_delete(&github).await;
    let w = world_with_run(&github, finished_run());
    assert_refused(&w, &github, "geschützt").await;
}

#[tokio::test]
async fn the_default_branch_stays() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, BRANCH).await;
    repo_and_branch(&github, BRANCH, false).await;
    allow_delete(&github).await;
    let w = world_with_run(&github, finished_run());
    assert_refused(&w, &github, "Standard-Branch").await;
}

#[tokio::test]
async fn a_branch_outside_the_pattern_of_the_repo_stays() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, "feature/BAU-142-x").await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/branches/feature/BAU-142-x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"protected": false})))
        .mount(&github)
        .await;
    let mut run = finished_run();
    run.target_branch = "feature/BAU-142-x".into();
    let w = world_with_run(&github, run);
    assert_refused(&w, &github, "Muster").await;
}

#[tokio::test]
async fn the_branch_of_another_pull_request_head_stays() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, "claude/BAU-9-other").await;
    repo_and_branch(&github, "main", false).await;
    allow_delete(&github).await;
    let w = world_with_run(&github, finished_run());
    assert_refused(&w, &github, "gehört nicht").await;
}

#[tokio::test]
async fn a_branch_that_is_already_gone_is_reported() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, BRANCH).await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&github)
        .await;
    let w = world_with_run(&github, finished_run());
    assert_refused(&w, &github, "nicht mehr").await;
}

#[tokio::test]
async fn runs_that_own_no_branch_are_refused_without_asking_github() {
    let github = MockServer::start().await;
    let mut no_pr = finished_run();
    no_pr.pull_request = None;
    let mut review = finished_run();
    review.kind = cockpit_core::RunKind::ReviewPr {
        review: cockpit_core::PrKey::new("firma/app", 31),
    };
    for run in [no_pr, review] {
        let w = world_with_run(&github, run);
        let key = w.ws().runs[0].key();
        assert!(!w.state.branch_check(&key, "").await.allowed);
        assert!(w.state.branch_delete(&key, "").await.is_err());
    }
    assert!(github.received_requests().await.unwrap().is_empty());
    let w = world_with_run(&github, finished_run());
    assert!(w.state.branch_delete("BAU-999", "").await.is_err());
}

#[tokio::test]
async fn without_github_nothing_happens() {
    let w = world(true);
    w.state.inner.live.set_state_for_test({
        let mut ws = demo::workspace();
        ws.runs = vec![finished_run()];
        ws
    });
    assert!(!w.state.branch_check("BAU-142", "").await.allowed);
    assert!(w.state.branch_delete("BAU-142", "").await.is_err());
}

// ---------- Marking the run as done ----------

async fn clone_dir(w: &World) -> PathBuf {
    let dir = w.state.agent_dir().join("runs").join("BAU-142");
    tokio::fs::create_dir_all(dir.join("run")).await.unwrap();
    tokio::fs::write(dir.join("run/file.txt"), "x")
        .await
        .unwrap();
    dir
}

#[tokio::test]
async fn a_merged_pull_request_marks_the_run_and_removes_its_clone() {
    let github = MockServer::start().await;
    pull(&github, "closed", true, BRANCH).await;
    let w = world_with_run(&github, finished_run());
    let dir = clone_dir(&w).await;
    w.state.reconcile_runs().await;
    let run = w.run();
    assert_eq!(run.closure, Some(Closure::Merged));
    assert!(!dir.exists(), "the clone goes away");
    let entry = w.ws().log.last().cloned().unwrap();
    assert_eq!(entry.actor, Actor::System);
    assert!(
        entry.text_in(Lang::De).contains("gemergt"),
        "{:?}",
        entry.text
    );
    // Asked once, then never again.
    let asked = github.received_requests().await.unwrap().len();
    w.state.reconcile_runs().await;
    assert_eq!(github.received_requests().await.unwrap().len(), asked);
    assert!(
        deletes(&github).await.is_empty(),
        "never deletes a branch itself"
    );
}

#[tokio::test]
async fn a_closed_pull_request_marks_the_run_as_closed() {
    let github = MockServer::start().await;
    pull(&github, "closed", false, BRANCH).await;
    let w = world_with_run(&github, finished_run());
    w.state.reconcile_runs().await;
    assert_eq!(w.run().closure, Some(Closure::Closed));
}

#[tokio::test]
async fn an_open_pull_request_changes_nothing() {
    let github = MockServer::start().await;
    pull(&github, "open", false, BRANCH).await;
    let w = world_with_run(&github, finished_run());
    let dir = clone_dir(&w).await;
    let before = w.ws();
    w.state.reconcile_runs().await;
    assert_eq!(w.ws(), before);
    assert!(dir.exists());
}

#[tokio::test]
async fn a_github_error_changes_nothing() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&github)
        .await;
    let w = world_with_run(&github, finished_run());
    let before = w.ws();
    w.state.reconcile_runs().await;
    assert_eq!(w.ws(), before);
}
