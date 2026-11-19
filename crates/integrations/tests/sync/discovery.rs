//! Pull requests without a list of repos: the three searches find them across everything the key
//! can see; configured repos stay "watched" and are loaded completely.

use super::open_pulls::{any_detail, list, pull, service};
use super::*;
use cockpit_core::sync::discovery::QUERIES;
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
        st.warnings.iter().any(|w| w.contains("acme/a")),
        "{:?}",
        st.warnings
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
        st.warnings.iter().any(|w| w.contains("search")),
        "{:?}",
        st.warnings
    );
}
