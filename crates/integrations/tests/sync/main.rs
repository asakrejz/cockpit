//! Sync against replayed responses (fixtures): fetching via the clients, merging
//! into the workspace, local state stays, errors become the state of the source.

use cockpit_core::{
    Action, Actor, Decision, Outcome, Problem, Remote, Source, Verdict, Workspace, sync,
};
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::{JiraClient, jql_for, jql_for_project};
use cockpit_integrations::{GitHubService, JiraService, Secret, Services};
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../fixtures/", $name))
    };
}

mod discovery;
mod fetch;
mod jira_details;
mod jira_scope;
mod jira_selection;
mod jira_sprints;
mod open_pulls;
mod outages;
mod pat_checks;
mod services;
mod two_repos;

const REPO: &str = "acme/baukasten";
/// 2024-05-14T12:00:00Z in milliseconds
const NOW_MS: u64 = 1_715_688_000_000;

fn ok(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(text.to_string(), "application/json")
}

/// The search answers "nothing found" (the sync searches in all repos besides its own list).
async fn search_none(s: &MockServer) {
    // Lowest priority: a test that mounts its own search answers goes first.
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ok(r#"{"total_count":0,"items":[]}"#))
        .with_priority(200)
        .mount(s)
        .await;
}

async fn get(s: &MockServer, p: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(response)
        .mount(s)
        .await;
}

/// GitHub with fixtures: #87 (review requested) and #91 (own PR, checks red) in the list of
/// open pull requests.
async fn github_server() -> MockServer {
    let s = github_server_without_checks().await;
    let base = format!("/repos/{REPO}");
    get(
        &s,
        &format!("{base}/commits/aaa111/check-runs"),
        ok(fixture!("github_check_runs_ok.json")),
    )
    .await;
    get(
        &s,
        &format!("{base}/commits/{}/check-runs", sha91()),
        ok(fixture!("github_check_runs_failed.json")),
    )
    .await;
    s
}

/// Like [`github_server`], but without any answer for the check runs (wiremock answers 404 then).
async fn github_server_without_checks() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    search_none(&s).await;
    let base = format!("/repos/{REPO}");
    get(
        &s,
        &format!("{base}/pulls"),
        ok(fixture!("github_pulls_open.json")),
    )
    .await;
    get(
        &s,
        &format!("{base}/pulls/87/files"),
        ok(fixture!("github_files_87.json")),
    )
    .await;
    get(&s, &format!("{base}/pulls/91/files"), ok("[]")).await;
    get(
        &s,
        &format!("{base}/issues/87/comments"),
        ok(fixture!("github_issue_comments_87.json")),
    )
    .await;
    get(&s, &format!("{base}/issues/91/comments"), ok("[]")).await;
    get(
        &s,
        &format!("{base}/pulls/87/comments"),
        ok(fixture!("github_review_comments_87.json")),
    )
    .await;
    get(&s, &format!("{base}/pulls/91/comments"), ok("[]")).await;
    get(
        &s,
        &format!("{base}/pulls/91/reviews"),
        ok(r#"[{"user":{"login":"tobias-lang"},"state":"CHANGES_REQUESTED","body":"Bitte Fokus prüfen","submitted_at":"2024-05-14T11:00:00Z"}]"#),
    )
    .await;
    get(&s, &format!("{base}/pulls/87/reviews"), ok("[]")).await;
    s
}

fn sha91() -> String {
    let v: serde_json::Value = serde_json::from_str(fixture!("github_pull_91.json")).unwrap();
    v["head"]["sha"].as_str().unwrap().to_string()
}

async fn jira_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(
        &s,
        "/rest/api/3/search/jql",
        ok(fixture!("jira_search.json")),
    )
    .await;
    get(
        &s,
        "/rest/api/3/issue/BAU-142/comment",
        ok(fixture!("jira_comments_142.json")),
    )
    .await;
    let other: serde_json::Value = serde_json::from_str(fixture!("jira_search.json")).unwrap();
    for i in other["issues"].as_array().unwrap() {
        let key = i["key"].as_str().unwrap();
        if key != "BAU-142" {
            get(
                &s,
                &format!("/rest/api/3/issue/{key}/comment"),
                ok(r#"{"comments":[]}"#),
            )
            .await;
        }
    }
    s
}

fn services(g: Option<&MockServer>, j: Option<&MockServer>) -> Services {
    Services {
        github: g.map(|s| GitHubService {
            client: GitHubClient::new(&s.uri(), Secret::new("ghp_x")).unwrap(),
            repos: vec![REPO.into()],
        }),
        jira: j.map(|s| JiraService {
            client: JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap(),
            jql: jql_for(&[], None).unwrap(),
            in_project: false,
            fields: Default::default(),
        }),
    }
}

async fn sync(d: &Services, ws: &Workspace, ms: u64) -> (Workspace, Remote) {
    let remote = d.fetch(ws, ms, &[]).await;
    let mut again = ws.clone();
    again
        .apply(
            Action::SyncRemote {
                remote: remote.clone(),
            },
            Actor::System,
            ms,
        )
        .unwrap();
    (again, remote)
}
