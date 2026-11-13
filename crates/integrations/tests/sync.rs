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
        include_str!(concat!("fixtures/", $name))
    };
}

const REPO: &str = "acme/baukasten";
/// 2024-05-14T12:00:00Z in milliseconds
const NOW_MS: u64 = 1_715_688_000_000;

fn ok(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(text.to_string(), "application/json")
}

async fn get(s: &MockServer, p: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(response)
        .mount(s)
        .await;
}

/// GitHub with fixtures: #87 (review requested) and #91 (own PR, checks red).
async fn github_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false review-requested:@me repo:{REPO}"),
        ))
        .respond_with(ok(fixture!("github_search_review.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false author:@me repo:{REPO}"),
        ))
        .respond_with(ok(fixture!("github_search_own.json")))
        .mount(&s)
        .await;
    let base = format!("/repos/{REPO}");
    get(
        &s,
        &format!("{base}/pulls/87"),
        ok(fixture!("github_pull_87.json")),
    )
    .await;
    get(
        &s,
        &format!("{base}/pulls/91"),
        ok(fixture!("github_pull_91.json")),
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

#[tokio::test]
async fn fetches_pull_requests_tickets_comments_checks_and_diffs() {
    let g = github_server().await;
    let j = jira_server().await;
    let d = services(Some(&g), Some(&j));
    let (ws, _) = sync(&d, &cockpit_core::demo::workspace(), NOW_MS).await;

    // Sample data is gone, real entries are there.
    assert!(ws.item("bau142").is_none() && ws.item("pr87").is_none());
    let pr87 = ws.item("gh-acme-baukasten-87").expect("PR 87");
    assert_eq!(pr87.pr.as_ref().unwrap().number, 87);
    assert_eq!(pr87.comments.len(), 4, "conversation and line comments");
    let pr91 = ws.item("gh-acme-baukasten-91").expect("PR 91");
    assert!(
        pr91.pr.as_ref().unwrap().changes_requested,
        "Urteil von mkeller"
    );
    assert!(
        pr91.comments
            .iter()
            .any(|c| c.text.starts_with("Review: Änderungen angefordert")),
    );
    assert_eq!(
        pr91.pr.as_ref().unwrap().checks,
        cockpit_core::Checks::Failed
    );
    let jira = ws.item("bau-142").expect("BAU-142");
    assert_eq!(jira.source, Source::Jira);
    assert!(!jira.comments.is_empty());

    // Diff and single checks in the review, mapping number → repo.
    let review = ws.review(87).unwrap();
    assert!(!review.files.is_empty());
    assert!(!review.check_runs.is_empty());
    assert_eq!(ws.review(91).unwrap().check_runs.len(), 3);
    assert_eq!(ws.sync.pr(87).unwrap().repo, REPO);
    assert_eq!(ws.sync.pr(87).unwrap().head_sha, "aaa111");
    assert!(ws.sync.state_of(Source::GitHub).unwrap().is_ok());
    assert!(ws.sync.state_of(Source::Jira).unwrap().is_ok());
    // The log records the new entry with author System.
    assert!(ws.log.iter().all(|e| e.actor == Actor::System));
    assert!(!ws.log.is_empty());
}

#[tokio::test]
async fn local_state_stays_on_the_next_sync() {
    let g = github_server().await;
    let j = jira_server().await;
    let d = services(Some(&g), Some(&j));
    let (mut ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    // Local: done, snoozed, review draft, verdict, agent run.
    ws.mark_done("bau-142");
    ws.snooze("gh-acme-baukasten-91", None);
    let line = ws.review(87).unwrap().files[0]
        .lines
        .iter()
        .position(|l| l.kind == cockpit_core::LineKind::Add)
        .unwrap();
    let file = ws.review(87).unwrap().files[0].name.clone();
    ws.add_line_comment(87, &file, line, "Eigener Entwurf", Some("neu"))
        .unwrap();
    ws.set_verdict(87, Verdict::Approve);
    ws.reviews
        .iter_mut()
        .find(|r| r.number == 87)
        .unwrap()
        .suggestions
        .push(cockpit_core::Suggestion::new(
            "s1",
            "x",
            cockpit_core::Level::Note,
            "Hm",
        ));
    ws.decide(87, "s1", Some(Decision::Declined));
    ws.start_agent("BAU-142", cockpit_core::AgentOptions::default())
        .unwrap();

    let (after, _) = sync(&d, &ws, NOW_MS + 120_000).await;
    assert!(after.item("bau-142").unwrap().read);
    assert!(after.snooze_of("gh-acme-baukasten-91").is_some());
    let r = after.review(87).unwrap();
    assert_eq!(r.line_comments.len(), 1);
    assert_eq!(r.line_comments[0].text, "Eigener Entwurf");
    assert_eq!(r.verdict, Verdict::Approve);
    assert_eq!(r.suggestions[0].decision, Some(Decision::Declined));
    assert_eq!(after.runs.len(), 1);
}

#[tokio::test]
async fn unchanged_pr_does_not_reload_the_diff() {
    let g = github_server().await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let before = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/pulls/87/files"))
        .count();
    let (_, remote) = sync(&d, &ws, NOW_MS + 1).await;
    let after = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/pulls/87/files"))
        .count();
    assert_eq!((before, after), (1, 1), "gleicher Commit: kein neuer Abruf");
    // And the diff is still in the remote.
    let Outcome::Loaded(l) = &remote.sources[0].outcome else {
        panic!("geladen erwartet")
    };
    assert!(
        !l.reviews
            .iter()
            .find(|r| r.number == 87)
            .unwrap()
            .files
            .is_empty()
    );
}

#[tokio::test]
async fn remote_disappears() {
    let g = github_server().await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert!(ws.item("gh-acme-baukasten-91").is_some());
    // At the service only #87 is left.
    g.reset().await;
    let g2 = github_server().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false author:@me repo:{REPO}"),
        ))
        .respond_with(ok(r#"{"items":[]}"#))
        .with_priority(1)
        .mount(&g2)
        .await;
    let d2 = services(Some(&g2), None);
    let (after, _) = sync(&d2, &ws, NOW_MS + 1).await;
    assert!(after.item("gh-acme-baukasten-91").is_none());
    assert!(after.review(91).is_none() && after.sync.pr(91).is_none());
    assert!(after.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn errors_become_the_source_state_and_nothing_is_lost() {
    let g = github_server().await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    let broken = |response: ResponseTemplate| async move {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(response)
            .mount(&s)
            .await;
        s
    };
    // 401: token expired.
    let s = broken(ResponseTemplate::new(401)).await;
    let (after, _) = sync(&services(Some(&s), None), &ws, NOW_MS + 1).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::Unauthorized)
    );
    assert!(after.item("gh-acme-baukasten-87").is_some(), "Stand bleibt");

    // Quota: pause until reset.
    let s = broken(
        ResponseTemplate::new(403)
            .insert_header("x-ratelimit-remaining", "0")
            .insert_header("retry-after", "90"),
    )
    .await;
    let (after, _) = sync(&services(Some(&s), None), &ws, NOW_MS + 2).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::RateLimited {
            until_ms: Some(NOW_MS + 2 + 90_000)
        })
    );
    assert_eq!(
        after.sync.paused_until(Source::GitHub, NOW_MS + 3),
        Some(NOW_MS + 2 + 90_000)
    );
    // Paused: the source is not even queried.
    let empty = services(Some(&s), None)
        .fetch(&after, NOW_MS + 3, &[Source::GitHub])
        .await;
    assert!(empty.sources.is_empty());

    // Network gone.
    let gone = Services {
        github: Some(GitHubService {
            client: GitHubClient::new("http://127.0.0.1:9", Secret::new("x")).unwrap(),
            repos: vec![REPO.into()],
        }),
        jira: None,
    };
    let (after, _) = sync(&gone, &ws, NOW_MS + 4).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::Offline)
    );
    assert!(after.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn unknown_repo_is_a_hint_not_an_outage() {
    let g = github_server().await;
    let d = Services {
        github: Some(GitHubService {
            client: GitHubClient::new(&g.uri(), Secret::new("x")).unwrap(),
            repos: vec!["gibt/es-nicht".into(), REPO.into()],
        }),
        jira: None,
    };
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            "is:pr is:open archived:false review-requested:@me repo:gibt/es-nicht",
        ))
        .respond_with(
            ResponseTemplate::new(422).set_body_json(json!({"message": "Validation Failed"})),
        )
        .mount(&g)
        .await;
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let st = ws.sync.state_of(Source::GitHub).unwrap();
    assert!(st.is_ok());
    assert!(
        st.warnings[0].contains("gibt/es-nicht"),
        "{:?}",
        st.warnings
    );
    assert!(ws.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn jira_is_read_page_by_page() {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    let issue = |n: u32| {
        json!({"key": format!("ABC-{n}"), "fields": {"summary": format!("T{n}"),
            "status": {"name": "To Do", "statusCategory": {"key": "new"}},
            "reporter": {"accountId": "x", "displayName": "Lena Koch"}, "updated": "2024-05-14T10:00:00Z"}})
    };
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("nextPageToken", "seite2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [issue(3)]})))
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"issues": [issue(1), issue(2)], "nextPageToken": "seite2"})),
        )
        .mount(&s)
        .await;
    for n in 1..=3 {
        get(
            &s,
            &format!("/rest/api/3/issue/ABC-{n}/comment"),
            ok(r#"{"comments":[]}"#),
        )
        .await;
    }
    let (ws, _) = sync(&services(None, Some(&s)), &Workspace::default(), NOW_MS).await;
    let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
}

#[tokio::test]
async fn jira_in_project_mode() {
    let j = jira_server().await;
    let mut d = services(None, Some(&j));
    if let Some(jira) = d.jira.as_mut() {
        jira.jql = jql_for_project("BAU").unwrap();
        jira.in_project = true;
    }
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let mine = ws.item_by_key("BAU-142").unwrap();
    assert_eq!(mine.reason_text, "Dir zugewiesen");
    let other: Vec<_> = ws.items.iter().filter(|i| i.key != "BAU-142").collect();
    assert!(!other.is_empty());
    assert!(
        other.iter().all(|i| i.reason_text == "Im Projekt"),
        "{other:?}"
    );
}

#[tokio::test]
async fn github_comments_page_by_page() {
    // 100 comments on page 1, one on page 2.
    let g = github_server().await;
    let entry = |i: u32| json!({"user": {"login": "mkeller"}, "body": format!("k{i}"), "created_at": "2024-05-14T09:00:00Z"});
    let page1: Vec<_> = (0..100).map(entry).collect();
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/issues/91/comments")))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([entry(100)])))
        .with_priority(1)
        .mount(&g)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/issues/91/comments")))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1))
        .with_priority(1)
        .mount(&g)
        .await;
    let (ws, _) = sync(&services(Some(&g), None), &Workspace::default(), NOW_MS).await;
    assert_eq!(
        ws.item("gh-acme-baukasten-91").unwrap().comments.len(),
        101 + 1
    );
}

#[test]
fn jql_from_projects_and_own_query() {
    assert_eq!(
        jql_for(&["BAU".into(), "WEB".into()], None).unwrap(),
        "project in (BAU, WEB) AND (assignee = currentUser() OR watcher = currentUser()) AND statusCategory != Done ORDER BY updated DESC"
    );
    assert_eq!(
        jql_for(&["BAU".into()], Some(" project = X ")).unwrap(),
        "project = X"
    );
    assert!(jql_for(&["BAU) OR (x".into()], None).is_err());
    assert!(jql_for(&[], None).unwrap().contains("currentUser()"));
}

#[test]
fn merge_stays_pure_and_deterministic() {
    let a = sync::merge(&Workspace::default(), &Remote::default(), 1);
    let b = sync::merge(&Workspace::default(), &Remote::default(), 1);
    assert_eq!(a, b);
}

// ---------- Two repos with the same pull request number ----------

const REPO_B: &str = "acme/web";

/// Both repos have a pull request #87, with different titles and different heads.
async fn two_repo_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    for (repo, title, sha) in [
        (REPO, "App: Avatar", "aaa111"),
        (REPO_B, "Web: Header", "ccc333"),
    ] {
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .and(query_param(
                "q",
                format!("is:pr is:open archived:false review-requested:@me repo:{repo}"),
            ))
            .respond_with(ok(&json!({"total_count":1,"items":[{
                "number": 87,
                "title": title,
                "state": "open",
                "repository_url": format!("https://api.github.com/repos/{repo}"),
                "pull_request": {"url": format!("https://api.github.com/repos/{repo}/pulls/87")}
            }]})
            .to_string()))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .and(query_param(
                "q",
                format!("is:pr is:open archived:false author:@me repo:{repo}"),
            ))
            .respond_with(ok(r#"{"total_count":0,"items":[]}"#))
            .mount(&s)
            .await;
        let base = format!("/repos/{repo}");
        get(
            &s,
            &format!("{base}/pulls/87"),
            ok(&json!({
                "number": 87, "state": "open", "title": title, "body": "",
                "draft": false, "user": {"login": "tobias-lang"},
                "head": {"ref": "feature/x", "sha": sha},
                "base": {"ref": "main", "sha": "bbb222"},
                "created_at": "2024-05-13T08:00:00Z", "updated_at": "2024-05-14T10:00:00Z",
                "labels": []
            })
            .to_string()),
        )
        .await;
        get(&s, &format!("{base}/pulls/87/files"), ok("[]")).await;
        get(&s, &format!("{base}/issues/87/comments"), ok("[]")).await;
        get(&s, &format!("{base}/pulls/87/comments"), ok("[]")).await;
        get(&s, &format!("{base}/pulls/87/reviews"), ok("[]")).await;
        get(
            &s,
            &format!("{base}/commits/{sha}/check-runs"),
            ok(fixture!("github_check_runs_ok.json")),
        )
        .await;
    }
    s
}

fn two_repo_services(g: &MockServer) -> Services {
    Services {
        github: Some(GitHubService {
            client: GitHubClient::new(&g.uri(), Secret::new("ghp_x")).unwrap(),
            repos: vec![REPO.into(), REPO_B.into()],
        }),
        jira: None,
    }
}

#[tokio::test]
async fn equal_pull_request_numbers_from_two_repos_both_arrive() {
    let g = two_repo_server().await;
    let d = two_repo_services(&g);
    let (ws, remote) = sync(&d, &Workspace::default(), NOW_MS).await;

    let (a, b) = (
        cockpit_core::PrKey::new(REPO, 87),
        cockpit_core::PrKey::new(REPO_B, 87),
    );
    assert_eq!(ws.reviews.len(), 2, "no pull request is skipped");
    assert_eq!(ws.review(&a).unwrap().title, "App: Avatar");
    assert_eq!(ws.review(&b).unwrap().title, "Web: Header");
    assert_eq!(ws.review(&a).unwrap().repo, REPO);
    assert_eq!(ws.sync.pr(&a).unwrap().head_sha, "aaa111");
    assert_eq!(ws.sync.pr(&b).unwrap().head_sha, "ccc333");
    // Entries have different IDs and each knows its repo.
    let items: Vec<_> = ws.items.iter().filter_map(|i| i.pr.as_ref()).collect();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|p| p.key() == a) && items.iter().any(|p| p.key() == b));
    assert!(
        ws.sync
            .state_of(Source::GitHub)
            .unwrap()
            .warnings
            .is_empty()
    );
    let Outcome::Loaded(loaded) = &remote.sources[0].outcome else {
        panic!("loaded")
    };
    assert_eq!(loaded.prs.len(), 2);

    // Local state of one repo's #87 survives the next sync and never touches the other one.
    let mut ws = ws;
    ws.apply(
        Action::SetVerdict {
            review: b.clone(),
            verdict: Verdict::Approve,
        },
        Actor::User,
        NOW_MS + 1,
    )
    .unwrap();
    let (ws, _) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert_eq!(ws.review(&b).unwrap().verdict, Verdict::Approve);
    assert_ne!(ws.review(&a).unwrap().verdict, Verdict::Approve);
}
