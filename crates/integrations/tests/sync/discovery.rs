//! Pull requests without a list of repos: the three searches find them across everything the key
//! can see; configured repos stay "watched" and are loaded completely.

use super::open_pulls::{any_detail, list, pull, service};
use super::*;
use cockpit_core::sync::discovery::QUERIES;
use cockpit_i18n::Lang;
use cockpit_integrations::github::PrLimits;

fn hit(repo: &str, n: u32) -> serde_json::Value {
    json!({
        "number": n, "title": format!("Change {n}"), "state": "open",
        "repository_url": format!("https://api.github.com/repos/{repo}"),
        "pull_request": { "url": format!("https://api.github.com/repos/{repo}/pulls/{n}") }
    })
}

async fn search(s: &MockServer, query: &str, items: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param("q", query))
        .respond_with(ok(
            &json!({"total_count": items.len(), "items": items}).to_string()
        ))
        .mount(s)
        .await;
}

async fn single(s: &MockServer, repo: &str, n: u32, author: &str) {
    get(
        s,
        &format!("/repos/{repo}/pulls/{n}"),
        ok(&pull(n, author, json!({})).to_string()),
    )
    .await;
}

async fn calls(s: &MockServer, p: &str) -> usize {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == p)
        .count()
}

#[tokio::test]
async fn without_any_repo_the_searches_find_the_pull_requests_once_each() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    search(&s, QUERIES[0], vec![hit("acme/app", 1)]).await;
    search(&s, QUERIES[1], vec![hit("acme/web", 2)]).await;
    // "Involved" repeats both and adds a third one.
    search(
        &s,
        QUERIES[2],
        vec![hit("acme/app", 1), hit("acme/web", 2), hit("acme/api", 3)],
    )
    .await;
    single(&s, "acme/app", 1, "mkeller").await;
    single(&s, "acme/web", 2, "lena").await;
    single(&s, "acme/api", 3, "lena").await;
    let d = service(&s, &[], PrLimits::default());

    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let mut repos: Vec<_> = ws.reviews.iter().map(|r| r.repo.clone()).collect();
    repos.sort();
    assert_eq!(repos, ["acme/api", "acme/app", "acme/web"]);
    assert!(ws.sync.state_of(Source::GitHub).unwrap().is_ok());
    // Three searches, and every pull request was loaded once although it was found twice.
    assert_eq!(calls(&s, "/search/issues").await, 3);
    assert_eq!(calls(&s, "/repos/acme/app/pulls/1").await, 1);
    // The relation comes from the pull request: mine is mine, the colleague's is team.
    let mine = ws
        .items
        .iter()
        .find(|i| i.pr.as_ref().is_some_and(|p| p.repo == "acme/app"))
        .unwrap();
    assert!(mine.is_mine());
}

#[tokio::test]
async fn watched_repos_load_completely_and_are_not_fetched_again_from_the_search() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    list(
        &s,
        REPO,
        1,
        vec![pull(1, "lena", json!({})), pull(2, "lena", json!({}))],
    )
    .await;
    search(&s, QUERIES[0], vec![hit(REPO, 1), hit("acme/web", 9)]).await;
    search(&s, QUERIES[1], vec![]).await;
    search(&s, QUERIES[2], vec![]).await;
    single(&s, "acme/web", 9, "mkeller").await;
    let d = service(&s, &[REPO], PrLimits::default());

    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.reviews.len(), 3, "two watched plus one found by search");
    assert!(ws.reviews.iter().any(|r| r.repo == "acme/web"));
    assert_eq!(calls(&s, &format!("/repos/{REPO}/pulls/1")).await, 0);
}

#[tokio::test]
async fn the_limits_bound_what_the_search_loads() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    search(
        &s,
        QUERIES[0],
        vec![
            hit("acme/a", 1),
            hit("acme/a", 2),
            hit("acme/b", 3),
            hit("acme/c", 4),
        ],
    )
    .await;
    search(&s, QUERIES[1], vec![]).await;
    search(&s, QUERIES[2], vec![]).await;
    for (repo, n) in [("acme/a", 1), ("acme/a", 2), ("acme/b", 3), ("acme/c", 4)] {
        single(&s, repo, n, "lena").await;
    }
    let d = service(
        &s,
        &[],
        PrLimits {
            per_repo: 1,
            total: 2,
        },
    );
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.reviews.len(), 2);
    let st = ws.sync.state_of(Source::GitHub).unwrap();
    assert!(st.is_ok());
    assert!(
        st.warning_lines(Lang::De)
            .iter()
            .any(|w| w.contains("acme/a")),
        "{:?}",
        st.warning_lines(Lang::De)
    );
    assert_eq!(calls(&s, "/repos/acme/c/pulls/4").await, 0);
}

#[tokio::test]
async fn a_failed_search_keeps_the_known_pull_requests_and_says_so() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    search(&s, QUERIES[0], vec![hit("acme/web", 2)]).await;
    search(&s, QUERIES[1], vec![]).await;
    search(&s, QUERIES[2], vec![]).await;
    single(&s, "acme/web", 2, "mkeller").await;
    let d = service(&s, &[], PrLimits::default());
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(ws.reviews.len(), 1);

    // The next tick: the search is rejected.
    let down = MockServer::start().await;
    any_detail(&down).await;
    get(
        &down,
        "/search/issues",
        ResponseTemplate::new(422)
            .set_body_raw(r#"{"message":"Validation Failed"}"#, "application/json"),
    )
    .await;
    let d = service(&down, &[], PrLimits::default());
    let (ws, _) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert_eq!(ws.reviews.len(), 1, "the pull request does not vanish");
    let st = ws.sync.state_of(Source::GitHub).unwrap();
    assert!(
        st.warning_lines(Lang::De)
            .iter()
            .any(|w| w.contains("Suche")),
        "{:?}",
        st.warning_lines(Lang::De)
    );
}

#[tokio::test]
async fn a_watched_repo_and_the_search_across_owners_show_up_together() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    // All open pull requests of the watched repo, colleagues' included …
    list(
        &s,
        REPO,
        1,
        vec![pull(1, "lena", json!({})), pull(2, "lena", json!({}))],
    )
    .await;
    // … and what concerns me elsewhere, in repos of two other owners.
    search(&s, QUERIES[0], vec![hit("acme/web", 5)]).await;
    search(&s, QUERIES[1], vec![hit("other-org/tool", 6)]).await;
    search(
        &s,
        QUERIES[2],
        vec![hit("other-org/tool", 6), hit("me/private", 7)],
    )
    .await;
    single(&s, "acme/web", 5, "mkeller").await;
    single(&s, "other-org/tool", 6, "lena").await;
    single(&s, "me/private", 7, "mkeller").await;
    let d = service(&s, &[REPO], PrLimits::default());

    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let mut repos: Vec<_> = ws.reviews.iter().map(|r| r.repo.as_str()).collect();
    repos.sort_unstable();
    repos.dedup();
    assert_eq!(repos, [REPO, "acme/web", "me/private", "other-org/tool"]);
    assert_eq!(ws.reviews.len(), 5);
}

#[tokio::test]
async fn the_sync_is_not_narrowed_by_a_jira_project_or_the_review_filter() {
    let s = MockServer::start().await;
    any_detail(&s).await;
    search(&s, QUERIES[0], vec![hit("acme/app", 1), hit("acme/web", 2)]).await;
    search(&s, QUERIES[1], vec![hit("acme/api", 3)]).await;
    search(&s, QUERIES[2], vec![]).await;
    single(&s, "acme/app", 1, "mkeller").await;
    single(&s, "acme/web", 2, "mkeller").await;
    single(&s, "acme/api", 3, "lena").await;
    let d = service(&s, &[], PrLimits::default());
    // The user looks at one Jira board and one repo.
    let known = Workspace {
        jira_project: Some("BAU".into()),
        review_filter: cockpit_core::ReviewFilter {
            repos: vec!["acme/app".into()],
            ..Default::default()
        },
        ..Workspace::default()
    };

    let (ws, _) = sync(&d, &known, NOW_MS).await;
    assert_eq!(
        ws.reviews.len(),
        3,
        "every repo is loaded, only the list is filtered"
    );
    let shown = ws.reviews_filtered(&ws.review_filter, None);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].repo, "acme/app");
}
