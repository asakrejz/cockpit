//! Can the access see the configured repos? (wiremock only, never a real service)

use super::*;
use serde_json::json;

const REPOS: &[&str] = &["acme/app", "acme-org/web"];

/// A server with the given repos; the token and the sign-in are stored if given.
fn app_for(api: &str, token: Option<&str>, oauth: Option<&str>) -> Router {
    let secrets = Arc::new(MemoryStore::new());
    if let Some(t) = token {
        secrets.set("github", &Secret::new(t)).unwrap();
    }
    if let Some(t) = oauth {
        secrets
            .set(
                crate::secrets::GITHUB_OAUTH_TOKENS,
                &Secret::new(json!({"access": t}).to_string()),
            )
            .unwrap();
    }
    let sources = Sources {
        github: Some(crate::config::GitHubConfig {
            api_url: api.into(),
            repos: REPOS.iter().map(|r| (*r).to_string()).collect(),
        }),
        ..Sources::default()
    };
    let state = AppState::create(
        db::open_in_memory().unwrap(),
        secrets,
        8787,
        sources,
        Settings::default(),
    );
    router(state, PathBuf::from("/x"))
}

async fn report(app: &Router) -> serde_json::Value {
    let (status, body) = send_request(
        app,
        request("GET", "/api/github/access")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(TOKEN), "token in the answer: {body}");
    crate::german::german_json(&body)
}

async fn repo_answers(gh: &MockServer, repo: &str, code: u16) {
    let body = if code == 200 {
        json!({"full_name": repo, "default_branch": "main", "archived": false,
               "permissions": {"push": true}})
    } else {
        json!({"message": "x"})
    };
    Mock::given(method("GET"))
        .and(path(format!("/repos/{repo}")))
        .respond_with(ResponseTemplate::new(code).set_body_json(body))
        .mount(gh)
        .await;
}

async fn installations(gh: &MockServer, list: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/user/installations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"installations": list})))
        .mount(gh)
        .await;
}

fn states(a: &serde_json::Value) -> Vec<&str> {
    a["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["visibility"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn everything_reachable_is_no_problem() {
    let gh = MockServer::start().await;
    for r in REPOS {
        repo_answers(&gh, r, 200).await;
    }
    let a = report(&app_for(&gh.uri(), None, Some(TOKEN))).await;
    assert_eq!(a["connected"], true);
    assert_eq!(a["via_app"], true);
    assert_eq!(states(&a), ["visible", "visible"]);
    assert!(a["installed"].is_null());
    // Everything was read, nothing changed.
    let requests = gh.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn a_repo_the_app_cannot_see_is_named_with_the_install_page() {
    let gh = MockServer::start().await;
    repo_answers(&gh, "acme/app", 200).await;
    repo_answers(&gh, "acme-org/web", 404).await;
    installations(&gh, json!([{"id": 5, "app_slug": "cockpit-app"}])).await;
    let a = report(&app_for(&gh.uri(), None, Some(TOKEN))).await;
    assert_eq!(states(&a), ["visible", "not_visible"]);
    assert_eq!(a["installed"], true);
    // The mock is not github.com, so no address is built for it.
    assert!(a["install_url"].is_null());
}

#[tokio::test]
async fn an_app_that_is_installed_nowhere_says_so() {
    let gh = MockServer::start().await;
    for r in REPOS {
        repo_answers(&gh, r, 404).await;
    }
    installations(&gh, json!([])).await;
    let a = report(&app_for(&gh.uri(), None, Some(TOKEN))).await;
    assert_eq!(states(&a), ["not_visible", "not_visible"]);
    assert_eq!(a["installed"], false);
}

#[tokio::test]
async fn a_missing_permission_is_told_apart_from_a_missing_install() {
    let gh = MockServer::start().await;
    repo_answers(&gh, "acme/app", 403).await;
    repo_answers(&gh, "acme-org/web", 404).await;
    installations(&gh, json!([{"id": 5}])).await;
    let a = report(&app_for(&gh.uri(), None, Some(TOKEN))).await;
    assert_eq!(states(&a), ["no_permission", "not_visible"]);
}

#[tokio::test]
async fn a_failing_service_is_unknown_not_empty() {
    let gh = MockServer::start().await;
    repo_answers(&gh, "acme/app", 500).await;
    repo_answers(&gh, "acme-org/web", 200).await;
    let a = report(&app_for(&gh.uri(), Some(TOKEN), None)).await;
    assert_eq!(a["via_app"], false);
    assert_eq!(states(&a), ["unreachable", "visible"]);
    // A token has no installations to ask for.
    assert!(a["installed"].is_null());
}

#[tokio::test]
async fn without_a_connection_nothing_is_checked() {
    let gh = MockServer::start().await;
    let a = report(&app_for(&gh.uri(), None, None)).await;
    assert_eq!(a["connected"], false);
    assert_eq!(a["repos"], json!([]));
    assert!(gh.received_requests().await.unwrap().is_empty());
}

#[test]
fn the_install_page_only_exists_for_github_com() {
    use crate::setup::access::install_url;
    assert_eq!(
        install_url("https://api.github.com/", "cockpit-app").as_deref(),
        Some("https://github.com/apps/cockpit-app/installations/new")
    );
    assert_eq!(
        install_url("https://git.acme.test/api/v3", "cockpit-app"),
        None
    );
    assert_eq!(install_url("https://api.github.com", "../evil?x"), None);
}

#[tokio::test]
async fn without_configured_repos_the_report_is_just_connected_and_asks_nothing() {
    let gh = MockServer::start().await;
    let secrets = Arc::new(MemoryStore::new());
    secrets.set("github", &Secret::new(TOKEN)).unwrap();
    let sources = Sources {
        github: Some(crate::config::GitHubConfig {
            api_url: gh.uri(),
            repos: vec![],
        }),
        ..Sources::default()
    };
    let state = AppState::create(
        db::open_in_memory().unwrap(),
        secrets,
        8787,
        sources,
        Settings::default(),
    );
    let app = router(state, PathBuf::from("/x"));
    let r = report(&app).await;
    assert_eq!(r["connected"], true);
    assert_eq!(r["repos"], json!([]));
    assert!(gh.received_requests().await.unwrap().is_empty());
}
