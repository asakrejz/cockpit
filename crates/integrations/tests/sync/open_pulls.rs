//! All open pull requests of the repos (not only mine): pages, limits, relation to the user,
//! and what happens when a repo cannot be read.

use super::*;
use cockpit_core::{Scope, Source as Src};
use cockpit_integrations::github::PrLimits;
use wiremock::matchers::path_regex;

pub(super) fn pull(n: u32, author: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut p = json!({
        "number": n, "state": "open", "title": format!("Change {n}"), "body": "",
        "draft": false, "user": {"login": author},
        "head": {"ref": format!("topic-{n}"), "sha": format!("sha{n}")},
        "base": {"ref": "main", "sha": "base"},
        "created_at": "2024-05-13T08:00:00Z", "updated_at": "2024-05-14T10:00:00Z",
        "labels": [], "requested_reviewers": [], "assignees": []
    });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

/// The calls that belong to every pull request, answered with „nothing special“.
async fn details(s: &MockServer) {
    get(s, "/user", ok(fixture!("github_user.json"))).await;
    search_none(s).await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/.*/commits/.*/check-runs$"))
        .respond_with(ok(r#"{"check_runs":[]}"#))
        .mount(s)
        .await;
}

pub(super) async fn any_detail(s: &MockServer) {
    for tail in ["files", "comments", "reviews"] {
        Mock::given(method("GET"))
            .and(path_regex(format!(
                r"^/repos/.*/(pulls|issues)/\d+/{tail}$"
            )))
            .respond_with(ok("[]"))
            .mount(s)
            .await;
    }
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/.*/commits/.*/check-runs$"))
        .respond_with(ok(r#"{"check_runs":[]}"#))
        .mount(s)
        .await;
    get(s, "/user", ok(fixture!("github_user.json"))).await;
    search_none(s).await;
}

pub(super) async fn list(s: &MockServer, repo: &str, page: u32, pulls: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/repos/{repo}/pulls")))
        .and(query_param("state", "open"))
        .and(query_param("per_page", "100"))
        .and(query_param("page", page.to_string()))
        .respond_with(ok(&serde_json::Value::Array(pulls).to_string()))
        .mount(s)
        .await;
}

pub(super) fn service(s: &MockServer, repos: &[&str], limits: PrLimits) -> Services {
    Services {
        github: Some(GitHubService {
            client: GitHubClient::new(&s.uri(), Secret::new("x"))
                .unwrap()
                .with_limits(limits),
            repos: repos.iter().map(|r| r.to_string()).collect(),
        }),
        jira: None,
    }
}

#[tokio::test]
async fn pull_requests_without_a_relation_to_me_are_listed_but_stay_quiet() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    list(
        &s,
        REPO,
        1,
        vec![
            // No ticket key anywhere, from a colleague: still arrives.
            pull(1, "tobias-lang", json!({})),
            // Mine as the author, as a requested reviewer, as an assignee (the user is mkeller).
            pull(2, "MKeller", json!({})),
            pull(
                3,
                "lena",
                json!({"requested_reviewers": [{"login": "mkeller"}]}),
            ),
            pull(
                4,
                "lena",
                json!({"assignees": [{"login": "mkeller"}, {"login": "x"}]}),
            ),
        ],
    )
    .await;
    let d = service(&s, &[REPO], PrLimits::default());
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    let item = |n: u32| ws.item(&format!("gh-acme-baukasten-{n}")).unwrap();
    assert_eq!(ws.items.len(), 4);
    let (one, two, three, four) = (item(1), item(2), item(3), item(4));
    assert_eq!(one.scope, Scope::Team);
    assert!(!one.pr.as_ref().unwrap().mine);
    assert_eq!(one.pr.as_ref().unwrap().author, "tobias-lang");
    assert_eq!(one.reason_text, "Pull Request von tobias-lang");
    assert!(two.is_mine() && two.pr.as_ref().unwrap().mine);
    assert!(three.pr.as_ref().unwrap().requested_me && three.pr.as_ref().unwrap().review_requested);
    assert!(four.pr.as_ref().unwrap().assigned_me && four.is_mine());
    assert!(!three.pr.as_ref().unwrap().assigned_me);

    // Only the pull requests that concern me can raise attention; the colleague's never.
    let attention: Vec<_> = ws
        .attention(cockpit_core::Filter::All, NOW_MS)
        .iter()
        .map(|i| i.key.clone())
        .collect();
    assert_eq!(attention, ["#3"], "{attention:?}");
    // The start filter „Alle“ lists them all, the filter „Meine“ keeps the quiet list.
    let all = cockpit_core::InboxFilter::default();
    let mine = cockpit_core::InboxFilter {
        ownership: cockpit_core::Ownership::Mine,
        ..Default::default()
    };
    let keys = |f: &cockpit_core::InboxFilter| -> Vec<String> {
        ws.inbox(
            cockpit_core::Filter::All,
            cockpit_core::SourceFilter::All,
            "",
            NOW_MS,
            f,
        )
        .iter()
        .map(|i| i.key.clone())
        .collect()
    };
    assert_eq!(keys(&all).len(), 4);
    assert_eq!(keys(&mine), ["#3"]);
    // Colleagues' pull requests need no comment calls, but they have a review (diff) and checks.
    assert!(ws.review(1).is_some());
}

#[tokio::test]
async fn pages_and_the_per_repo_limit_say_what_was_left_out() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    let all: Vec<_> = (1..=205).map(|n| pull(n, "lena", json!({}))).collect();
    list(&s, REPO, 1, all[..100].to_vec()).await;
    list(&s, REPO, 2, all[100..200].to_vec()).await;
    list(&s, REPO, 3, all[200..].to_vec()).await;
    let d = service(
        &s,
        &[REPO],
        PrLimits {
            per_repo: 150,
            total: 500,
        },
    );
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.items.len(), 150);
    let st = ws.sync.state_of(Src::GitHub).unwrap();
    assert!(st.is_ok());
    assert_eq!(
        st.warnings,
        [format!("{REPO}: Nur die ersten 150 PRs je Repo geladen")]
    );

    // Exactly as many as the limit: nothing left out, no hint.
    let d = service(
        &s,
        &[REPO],
        PrLimits {
            per_repo: 205,
            total: 500,
        },
    );
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.items.len(), 205);
    assert!(ws.sync.state_of(Src::GitHub).unwrap().warnings.is_empty());
}

#[tokio::test]
async fn the_total_limit_holds_over_all_repos() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    for repo in [REPO, "acme/web"] {
        list(
            &s,
            repo,
            1,
            (1..=30).map(|n| pull(n, "lena", json!({}))).collect(),
        )
        .await;
    }
    let d = service(
        &s,
        &[REPO, "acme/web"],
        PrLimits {
            per_repo: 200,
            total: 40,
        },
    );
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.items.len(), 40);
    let st = ws.sync.state_of(Src::GitHub).unwrap();
    assert!(
        st.warnings.iter().any(|w| w.contains("40 PRs insgesamt")),
        "{:?}",
        st.warnings
    );
}

#[tokio::test]
async fn a_forbidden_or_missing_repo_does_not_block_the_other() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    list(&s, REPO, 1, vec![pull(1, "lena", json!({}))]).await;
    for (repo, status) in [("acme/secret", 403), ("acme/gone", 404)] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/{repo}/pulls")))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"message": "no"})))
            .mount(&s)
            .await;
    }
    let d = service(&s, &["acme/secret", REPO, "acme/gone"], PrLimits::default());
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let st = ws.sync.state_of(Src::GitHub).unwrap();
    assert!(st.is_ok(), "{st:?}");
    assert_eq!(ws.items.len(), 1);
    assert!(st.warnings.iter().any(|w| w.starts_with("acme/secret")));
    assert!(st.warnings.iter().any(|w| w.starts_with("acme/gone")));
}

#[tokio::test]
async fn a_repo_that_fails_for_a_moment_keeps_its_last_state() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    list(&s, REPO, 1, vec![pull(1, "lena", json!({}))]).await;
    list(&s, "acme/web", 1, vec![pull(7, "tobias-lang", json!({}))]).await;
    let d = service(&s, &[REPO, "acme/web"], PrLimits::default());
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.items.len(), 2);

    // The second repo answers 500 this time; the first one delivers a new state.
    let s2 = MockServer::start().await;
    any_detail(&s2).await;
    list(
        &s2,
        REPO,
        1,
        vec![pull(1, "lena", json!({})), pull(2, "lena", json!({}))],
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/web/pulls"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&s2)
        .await;
    let d2 = service(&s2, &[REPO, "acme/web"], PrLimits::default());
    let (after, _) = sync(&d2, &ws, NOW_MS + 1).await;
    let st = after.sync.state_of(Src::GitHub).unwrap();
    assert!(st.is_ok());
    assert!(after.item("gh-acme-web-7").is_some(), "stale, but not gone");
    assert!(after.item("gh-acme-baukasten-2").is_some());
    assert!(
        st.warnings.iter().any(|w| w.starts_with("acme/web")),
        "{:?}",
        st.warnings
    );
}

#[tokio::test]
async fn the_rate_limit_stops_the_source_and_keeps_its_data() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    list(&s, REPO, 1, vec![pull(1, "lena", json!({}))]).await;
    let (ws, _) = sync(
        &service(&s, &[REPO], PrLimits::default()),
        &Workspace::default(),
        NOW_MS,
    )
    .await;

    let limited = MockServer::start().await;
    get(&limited, "/user", ok(fixture!("github_user.json"))).await;
    search_none(&limited).await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/pulls")))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("retry-after", "60"),
        )
        .mount(&limited)
        .await;
    let (after, _) = sync(
        &service(&limited, &[REPO], PrLimits::default()),
        &ws,
        NOW_MS + 1,
    )
    .await;
    assert!(matches!(
        after.sync.state_of(Src::GitHub).unwrap().problem,
        Some(Problem::RateLimited { .. })
    ));
    assert!(after.item("gh-acme-baukasten-1").is_some());
}

#[tokio::test]
async fn details_are_not_loaded_for_pull_requests_of_colleagues() {
    let s = MockServer::start().await;
    details(&s).await;
    list(&s, REPO, 1, vec![pull(1, "lena", json!({}))]).await;
    // Only files are allowed besides the checks: comments and reviews would be a 404 here.
    get(&s, &format!("/repos/{REPO}/pulls/1/files"), ok("[]")).await;
    let (ws, remote) = sync(
        &service(&s, &[REPO], PrLimits::default()),
        &Workspace::default(),
        NOW_MS,
    )
    .await;
    assert_eq!(ws.items.len(), 1, "{remote:?}");
    assert!(ws.sync.state_of(Src::GitHub).unwrap().is_ok());
}
