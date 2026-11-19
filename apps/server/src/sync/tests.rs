use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use cockpit_core::{Workspace, persist};
use cockpit_integrations::Secret;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::app::router;
use crate::db;
use crate::secrets::{MemoryStore, SecretStore};

const TOKEN: &str = "ghp_streng_geheim_42";

fn sources(api: &str) -> Sources {
    Sources {
        github: Some(GitHubConfig {
            api_url: api.into(),
            repos: vec!["a/x".into()],
        }),
        jira: None,
        interval: Duration::from_secs(120),
        ..Sources::default()
    }
}

fn state(sources: Sources, with_token: bool) -> AppState {
    let secrets = Arc::new(MemoryStore::new());
    if with_token {
        secrets.set(secrets::GITHUB, &Secret::new(TOKEN)).unwrap();
    }
    let conn = db::open_in_memory().unwrap();
    AppState::with_sources(conn, secrets, 8787, sources)
}

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
}

async fn send_request(state: &AppState, req: Request<Body>) -> (StatusCode, String) {
    let app: Router = router(state.clone(), PathBuf::from("/x"));
    let answer = app.oneshot(req).await.unwrap();
    let status = answer.status();
    let body = answer.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn sync(state: &AppState) -> (StatusCode, serde_json::Value) {
    let req = request("POST", "/api/sync")
        .header("content-type", "application/json")
        .body(Body::empty())
        .unwrap();
    let (s, b) = send_request(state, req).await;
    (s, serde_json::from_str(&b).unwrap_or(json!({"roh": b})))
}

async fn action(state: &AppState, action: serde_json::Value) -> (StatusCode, String) {
    let req = request("POST", "/api/actions")
        .header("content-type", "application/json")
        .body(Body::from(json!({"action": action}).to_string()))
        .unwrap();
    send_request(state, req).await
}

async fn snapshot(state: &AppState) -> (Workspace, u64) {
    let req = request("GET", "/api/workspace")
        .body(Body::empty())
        .unwrap();
    let (_, body) = send_request(state, req).await;
    persist::load_with_revision(&body).unwrap()
}

async fn health(state: &AppState) -> serde_json::Value {
    let req = request("GET", "/api/health").body(Body::empty()).unwrap();
    serde_json::from_str(&send_request(state, req).await.1).unwrap()
}

fn ok(v: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(v)
}

async fn get(s: &MockServer, p: &str, a: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(a)
        .mount(s)
        .await;
}

/// GitHub with a pull request (#7, review requested, checks green).
async fn github() -> MockServer {
    let s = MockServer::start().await;
    mount_github(&s).await;
    s
}

async fn mount_github(s: &MockServer) {
    get(s, "/user", ok(json!({"login": "ich"}))).await;
    // The search finds nothing besides the repos of the list.
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ok(json!({"total_count": 0, "items": []})))
        .with_priority(200)
        .mount(s)
        .await;
    get(
        s,
        "/repos/a/x/pulls",
        ok(json!([{
        "number": 7, "title": "Avatar", "user": {"login": "lena"},
        "requested_reviewers": [{"login": "ich"}],
        "head": {"ref": "feature/x", "sha": "s1"}, "base": {"ref": "main"},
        "updated_at": "2024-05-14T10:00:00Z"}])),
    )
    .await;
    get(s, "/repos/a/x/pulls/7/files", ok(json!([
        {"filename": "a.rs", "patch": "@@ -1,2 +1,2 @@\n fn a() {\n-    alt();\n+    neu();\n }"}]))).await;
    get(s, "/repos/a/x/issues/7/comments", ok(json!([]))).await;
    get(s, "/repos/a/x/pulls/7/comments", ok(json!([]))).await;
    get(s, "/repos/a/x/pulls/7/reviews", ok(json!([]))).await;
    get(
        s,
        "/repos/a/x/commits/s1/check-runs",
        ok(json!({"check_runs": [
        {"id": 5, "name": "Tests", "status": "completed", "conclusion": "success"}]})),
    )
    .await;
}

fn jira_state(site: &str) -> AppState {
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::JIRA, &Secret::new("jira-geheim"))
        .unwrap();
    let sources = Sources {
        jira: Some(JiraConfig {
            site: site.into(),
            email: "ich@firma.example".into(),
            projects: vec!["ALT".into()],
            jql: None,
            hidden_fields: vec![],
            field_order: vec![],
            team_field: None,
        }),
        ..Sources::default()
    };
    AppState::with_sources(db::open_in_memory().unwrap(), secrets, 8787, sources)
}

#[tokio::test]
async fn jira_projects_for_the_selection() {
    // Without Jira: nothing to choose, but no error.
    let state = state(Sources::default(), false);
    let req = request("GET", "/api/jira/projects")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_request(&state, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["active"],
        false
    );

    let j = MockServer::start().await;
    get(
        &j,
        "/rest/api/3/project/search",
        ok(json!({"isLast": true, "values": [
            {"key": "BAU", "name": "Baukasten"}, {"key": "WEB", "name": "Webseite"}]})),
    )
    .await;
    let state = jira_state(&j.uri());
    let req = request("GET", "/api/jira/projects")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_request(&state, req).await;
    assert_eq!(status, StatusCode::OK);
    let answer: JiraProjectsResponse = serde_json::from_str(&body).unwrap();
    assert!(answer.active);
    assert_eq!(
        answer.projects,
        [
            JiraProject {
                key: "BAU".into(),
                name: "Baukasten".into()
            },
            JiraProject {
                key: "WEB".into(),
                name: "Webseite".into()
            },
        ]
    );
    assert!(!body.contains("jira-geheim"));
}

#[tokio::test]
async fn chosen_project_determines_the_query() {
    let j = MockServer::start().await;
    get(&j, "/rest/api/3/myself", ok(json!({"accountId": "ich"}))).await;
    let issue = json!({"key": "WEB-7", "fields": {"summary": "Fremdes Ticket",
        "status": {"name": "To Do", "statusCategory": {"key": "new"}},
        "assignee": {"accountId": "lena", "displayName": "Lena Koch"},
        "updated": "2024-05-14T10:00:00Z"}});
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param(
            "jql",
            "project = WEB AND sprint in openSprints() ORDER BY updated DESC",
        ))
        .respond_with(ok(json!({"issues": [issue]})))
        .expect(1)
        .mount(&j)
        .await;
    get(
        &j,
        "/rest/api/3/issue/WEB-7/comment",
        ok(json!({"comments": []})),
    )
    .await;

    let state = jira_state(&j.uri());
    let (status, _) = action(&state, json!({"SelectJiraProject": {"project": "WEB"}})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, answer) = sync(&state).await;
    assert_eq!((status, &answer["active"]), (StatusCode::OK, &json!(true)));
    let (ws, _) = snapshot(&state).await;
    let item = ws
        .item_by_key("WEB-7")
        .expect("ticket from the selected project");
    assert_eq!(item.reason_text, "Im Projekt");
    assert_eq!(ws.jira_project.as_deref(), Some("WEB"));
}

#[tokio::test]
async fn sync_without_source_lets_the_demo_data() {
    let state = state(Sources::default(), false);
    let (status, answer) = sync(&state).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["active"], false);
    let (ws, revision) = snapshot(&state).await;
    assert_eq!(ws, cockpit_core::demo::multi_repo_workspace());
    assert_eq!(revision, 0);
    let h = health(&state).await;
    assert_eq!(h["sync"]["github"], "off");
    assert_eq!(h["sync"]["jira"], "off");
}

#[tokio::test]
async fn source_without_key_stays_out() {
    let g = github().await;
    let state = state(sources(&g.uri()), false);
    let (_, answer) = sync(&state).await;
    assert_eq!(answer["active"], false);
    assert!(g.received_requests().await.unwrap().is_empty());
    assert_eq!(health(&state).await["sync"]["github"], "off");
}

#[tokio::test]
async fn post_sync_fetches_pull_requests_and_shows_the_state() {
    let g = github().await;
    let state = state(sources(&g.uri()), true);
    let (status, answer) = sync(&state).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["active"], true);
    assert_eq!(answer["revision"], 1);

    let (ws, _) = snapshot(&state).await;
    assert!(ws.item("gh-a-x-7").is_some());
    assert!(ws.item("bau142").is_none(), "sample data is gone");
    assert_eq!(ws.sync.pr(7).unwrap().repo, "a/x");
    assert_eq!(ws.review(7).unwrap().files[0].name, "a.rs");
    let h = health(&state).await;
    assert_eq!(h["sync"]["github"], "ok");
    assert_eq!(h["sync"]["jira"], "off");
    assert_eq!(h["sync"]["interval_s"], 120);
    // Every fetch carried the key – and only there.
    let requests = g.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.headers.get("authorization").is_some())
    );
    assert!(!persist::save(&ws).contains(TOKEN));
}

#[tokio::test]
async fn expired_token_is_to_state_and_nothing_goes_lost() {
    let g = github().await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    g.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&g)
        .await;
    let (status, _) = sync(&state).await;
    assert_eq!(status, StatusCode::OK);
    let (ws, _) = snapshot(&state).await;
    assert!(ws.item("gh-a-x-7").is_some());
    assert_eq!(health(&state).await["sync"]["github"], "unauthorized");
}

#[tokio::test]
async fn service_outage_and_recovery_keep_the_data() {
    let g = github().await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    g.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&g)
        .await;
    let (status, _) = sync(&state).await;
    assert_eq!(status, StatusCode::OK, "an outage is not an error of ours");
    let h = health(&state).await;
    assert_eq!(h["sync"]["github"], "error");
    assert!(h["sync"]["details"][0]["since_ms"].as_u64().is_some());
    assert!(h["sync"]["details"][0]["retry_at_ms"].is_null());
    let (ws, _) = snapshot(&state).await;
    assert!(ws.item("gh-a-x-7").is_some(), "last state stays");

    // The service is back: the next sync clears the problem.
    g.reset().await;
    mount_github(&g).await;
    sync(&state).await;
    let h = health(&state).await;
    assert_eq!(h["sync"]["github"], "ok");
    assert!(h["sync"]["details"][0]["since_ms"].is_null());
}

#[tokio::test]
async fn quota_pauses_the_sync() {
    let g = github().await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    g.reset().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("retry-after", "600"),
        )
        .mount(&g)
        .await;
    sync(&state).await;
    let h = health(&state).await;
    assert_eq!(h["sync"]["github"], "quota");
    let d = &h["sync"]["details"][0];
    assert_eq!(d["source"], "github");
    assert_eq!(d["status"], "quota");
    let retry = d["retry_at_ms"].as_u64().expect("retry time");
    let since = d["since_ms"].as_u64().expect("since");
    assert!(
        (590_000..=610_000).contains(&(retry - since)),
        "{retry} {since}"
    );
    assert!(
        d["last_ok_ms"].as_u64().is_some(),
        "last good state is known"
    );
    assert!(!h.to_string().contains(TOKEN));
    let before = g.received_requests().await.unwrap().len();
    // During the pause the next sync does not even ask.
    sync(&state).await;
    assert_eq!(g.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn answer_goes_first_at_github_then_is_it_local_applied() {
    let g = github().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/issues/7/comments"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {TOKEN}").as_str(),
        ))
        .and(wiremock::matchers::body_json(json!({"body": "Danke!"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
        .expect(1)
        .mount(&g)
        .await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    let (status, body) = action(
        &state,
        json!({"Reply": {"item": "gh-a-x-7", "text": "Danke!"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (ws, _) = snapshot(&state).await;
    let c = ws
        .item("gh-a-x-7")
        .unwrap()
        .comments
        .last()
        .unwrap()
        .clone();
    assert!(c.own && c.text == "Danke!");
    assert_eq!(ws.log.last().unwrap().actor, Actor::User);
}

#[tokio::test]
async fn fails_the_call_missing_changes_itself_nothing() {
    let g = github().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({"message": format!("Resource not accessible by token {TOKEN}")}),
            ),
        )
        .mount(&g)
        .await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    let (before, rev) = snapshot(&state).await;
    let (status, body) = action(&state, json!({"Reply": {"item": "gh-a-x-7", "text": "Hi"}})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        body.contains("GitHub hat die Aktion nicht ausgeführt"),
        "{body}"
    );
    assert!(body.contains("Es wurde nichts geändert"));
    assert!(!body.contains(TOKEN), "the key never shows up: {body}");
    let (afterwards, rev2) = snapshot(&state).await;
    assert_eq!((before, rev), (afterwards, rev2));

    // Rejection (422) gets the message of the service.
    g.reset().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(422).set_body_json(json!({"message": "Validation Failed"})),
        )
        .mount(&g)
        .await;
    let (status, body) = action(&state, json!({"Reply": {"item": "gh-a-x-7", "text": "Hi"}})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body.contains("Validation Failed"), "{body}");
}

#[tokio::test]
async fn rules_apply_before_the_call() {
    let g = github().await;
    // Merging without a sent review: the rules reject, GitHub hears nothing of it.
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&g)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&g)
        .await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    let (status, body) = action(
        &state,
        json!({"MergePr": {"review": 7, "method": "Squash"}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("Mergen geht noch nicht"));
    // The sync is a matter of the system, not of the interface.
    let (status, _) = action(&state, json!({"SyncRemote": {"remote": {}}})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn review_and_merge_go_after_github() {
    let g = github().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/pulls/7/reviews"))
        .and(wiremock::matchers::body_json(json!({
            "event": "APPROVE", "commit_id": "s1",
            "comments": [{"path": "a.rs", "line": 2, "side": "RIGHT",
                          "body": "Schöner so\n\n```suggestion\n    neuer();\n```"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .expect(1)
        .mount(&g)
        .await;
    Mock::given(method("PUT"))
        .and(path("/repos/a/x/pulls/7/merge"))
        .and(wiremock::matchers::body_json(
            json!({"merge_method": "squash", "sha": "s1"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"merged": true})))
        .expect(1)
        .mount(&g)
        .await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    // Diff: context, removed, new, context → index 2 is the new line (no. 2).
    let (s, b) = action(&state, json!({"AddLineComment": {
        "review": 7, "file": "a.rs", "line": 2, "text": "Schöner so", "suggestion": "    neuer();"}})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    action(
        &state,
        json!({"SetVerdict": {"review": 7, "verdict": "Approve"}}),
    )
    .await;
    let (s, b) = action(&state, json!({"SendReview": {"review": 7, "comment": ""}})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = action(
        &state,
        json!({"MergePr": {"review": 7, "method": "Squash"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (ws, _) = snapshot(&state).await;
    assert!(ws.review(7).unwrap().sent && ws.review(7).unwrap().merged.is_some());
}

#[tokio::test]
async fn other_actions_stay_local() {
    let g = github().await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    let before = g.received_requests().await.unwrap().len();
    let (s, _) = action(&state, json!({"MarkDone": {"item": "gh-a-x-7"}})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(g.received_requests().await.unwrap().len(), before);
}
mod agent_review;

/// GitHub without any configured repo: the pull request is only reachable through the search.
async fn github_search_only() -> MockServer {
    let s = MockServer::start().await;
    mount_github(&s).await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param("q", "is:pr is:open author:@me"))
        .respond_with(ok(json!({"total_count": 1, "items": [{
            "number": 7,
            "repository_url": "https://api.github.com/repos/a/x"
        }]})))
        .mount(&s)
        .await;
    get(
        &s,
        "/repos/a/x/pulls/7",
        ok(json!({
            "number": 7, "title": "Avatar", "user": {"login": "ich"},
            "requested_reviewers": [],
            "head": {"ref": "feature/x", "sha": "s1"}, "base": {"ref": "main"},
            "updated_at": "2024-05-14T10:00:00Z"})),
    )
    .await;
    s
}

fn sources_without_repos(api: &str) -> Sources {
    Sources {
        github: Some(GitHubConfig {
            api_url: api.into(),
            repos: vec![],
        }),
        interval: Duration::from_secs(120),
        ..Sources::default()
    }
}

#[tokio::test]
async fn connected_github_syncs_without_any_repo_and_health_says_so() {
    let g = github_search_only().await;
    let state = state(sources_without_repos(&g.uri()), true);
    // Connected means active: the sync is not "off" for a missing list of repos.
    assert_eq!(health(&state).await["sync"]["github"], "waiting");
    let (status, answer) = sync(&state).await;
    assert_eq!((status, &answer["active"]), (StatusCode::OK, &json!(true)));
    let (ws, _) = snapshot(&state).await;
    assert_eq!(ws.sync.pr(7).unwrap().repo, "a/x");
    assert_eq!(health(&state).await["sync"]["github"], "ok");
    // No list call for the repos was made: only the search and the pull request itself.
    let paths: Vec<String> = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(
        !paths.contains(&"/repos/a/x/pulls".to_string()),
        "{paths:?}"
    );
    assert_eq!(
        paths.iter().filter(|p| *p == "/search/issues").count(),
        3,
        "{paths:?}"
    );
}

#[tokio::test]
async fn an_unconnected_github_stays_off_without_a_section() {
    // No `[github]` section and no key: nothing is connected, so the sync is off.
    let unconnected = state(Sources::default(), false);
    assert_eq!(health(&unconnected).await["sync"]["github"], "off");
}
