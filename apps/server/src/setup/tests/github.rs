//! The repo list and the connection test of GitHub (wiremock only, never a real service).

use super::*;
use serde_json::json;

/// A server whose GitHub address is the mock; `token` is stored in the .env file if given.
fn gh_app(api: &str, token: Option<&str>, oauth: Option<&str>) -> Router {
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
    router(state, PathBuf::from("/x"))
}

async fn list(app: &Router) -> (StatusCode, serde_json::Value) {
    let (status, body) = send_request(
        app,
        request("GET", "/api/github/repos")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(!body.contains(TOKEN), "token in the answer: {body}");
    (status, german_or_default(&body))
}

/// The JSON of an answer with its messages in German; `null` if it is no JSON.
fn german_or_default(body: &str) -> serde_json::Value {
    serde_json::from_str(body)
        .map(|mut v| {
            crate::german::german_value(&mut v);
            v
        })
        .unwrap_or_default()
}

async fn test_connection(app: &Router, repos: &[&str]) -> serde_json::Value {
    let (status, body) = send_request(
        app,
        request("POST", "/api/github/check")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "repos": repos }).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(TOKEN), "token in the answer: {body}");
    crate::german::german_json(&body)
}

fn entry(name: &str) -> serde_json::Value {
    json!({"full_name": name, "private": true, "archived": false, "default_branch": "develop",
           "permissions": {"push": true}})
}

#[tokio::test]
async fn the_list_comes_from_the_user_repos_with_a_token() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([entry("firma/app")])))
        .mount(&gh)
        .await;
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let (status, a) = list(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(a["via"], "token");
    assert_eq!(a["truncated"], false);
    assert_eq!(a["repos"][0]["repo"], "firma/app");
    assert_eq!(a["repos"][0]["default_branch"], "develop");
    assert_eq!(a["repos"][0]["private"], true);
    assert_eq!(a["repos"][0]["can_push"], true);
    let requests = gh.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn the_list_is_kept_for_a_few_minutes_so_the_dialogs_do_not_ask_again() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([entry("firma/app")])))
        .expect(1)
        .mount(&gh)
        .await;
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let (first, _) = list(&app).await;
    let (second, a) = list(&app).await;
    assert_eq!((first, second), (StatusCode::OK, StatusCode::OK));
    assert_eq!(a["repos"][0]["repo"], "firma/app");
    // `expect(1)` is checked when the mock server is dropped: one call to GitHub in all.
}

#[tokio::test]
async fn the_list_of_a_github_app_comes_from_its_installations() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/installations"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"installations": [{"id": 5}]})),
        )
        .mount(&gh)
        .await;
    Mock::given(method("GET"))
        .and(path("/user/installations/5/repositories"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"repositories": [entry("firma/web")]})),
        )
        .mount(&gh)
        .await;
    // The sign-in goes before a token that may also be there.
    let app = gh_app(&gh.uri(), Some("other"), Some(TOKEN));
    let (status, a) = list(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(a["via"], "app");
    assert_eq!(a["repos"][0]["repo"], "firma/web");
}

#[tokio::test]
async fn the_list_names_the_problem_in_plain_text() {
    for (code, status, word) in [
        (401u16, StatusCode::UNAUTHORIZED, "lehnt den Token ab"),
        (403, StatusCode::FORBIDDEN, "verweigert den Zugriff"),
        (404, StatusCode::NOT_FOUND, "kennt diese Adresse nicht"),
        (429, StatusCode::TOO_MANY_REQUESTS, "bremst"),
        (500, StatusCode::BAD_GATEWAY, "unerwartet"),
    ] {
        let gh = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/repos"))
            .respond_with(
                ResponseTemplate::new(code)
                    .set_body_json(json!({"message": format!("bad {TOKEN}")})),
            )
            .mount(&gh)
            .await;
        let app = gh_app(&gh.uri(), Some(TOKEN), None);
        let (s, body) = send_request(
            &app,
            request("GET", "/api/github/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(s, status, "{code}: {body}");
        assert!(
            crate::german::german(&body).contains(word),
            "{code}: {body}"
        );
        assert!(!body.contains(TOKEN), "{code}: {body}");
    }
}

#[tokio::test]
async fn the_list_without_a_connection_says_so() {
    let (app, _) = jira_state("", "", false);
    let (status, body) = send_request(
        &app,
        request("GET", "/api/github/repos")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        crate::german::german(&body).contains("noch nicht verbunden"),
        "{body}"
    );
}

#[tokio::test]
async fn the_connection_test_names_user_and_repos_and_only_reads() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "ada"})))
        .mount(&gh)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"default_branch": "main", "permissions": {"push": true}})),
        )
        .mount(&gh)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/weg"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&gh)
        .await;
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let a = test_connection(&app, &["firma/app", "firma/weg"]).await;
    assert_eq!(a["cause"], "Ok");
    assert_eq!(a["user"], "ada");
    assert!(a["message"].as_str().unwrap().contains("ada"));
    // The connection works, but one repo is not visible: not "ok" overall.
    assert_eq!(a["ok"], false);
    assert_eq!(a["repos"][0]["state"], "Ok");
    assert_eq!(a["repos"][1]["state"], "NotFound");
    assert!(
        a["repos"][1]["message"]
            .as_str()
            .unwrap()
            .contains("sieht es nicht")
    );
    let requests = gh.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn the_connection_test_maps_failures_of_the_user_call() {
    for (code, cause) in [
        (401u16, "Token"),
        (403, "Permission"),
        (429, "RateLimit"),
        (500, "Other"),
    ] {
        let gh = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(code))
            .mount(&gh)
            .await;
        let app = gh_app(&gh.uri(), Some(TOKEN), None);
        let a = test_connection(&app, &["firma/app"]).await;
        assert_eq!(a["cause"], cause, "{code}");
        assert_eq!(a["ok"], false);
        assert_eq!(a["repos"].as_array().unwrap().len(), 0);
    }
}

#[tokio::test]
async fn the_connection_test_without_token_and_with_a_dead_address() {
    let (app, _) = jira_state("", "", false);
    assert_eq!(test_connection(&app, &[]).await["cause"], "NotConnected");
    let app = gh_app("http://127.0.0.1:9", Some(TOKEN), None);
    assert_eq!(test_connection(&app, &[]).await["cause"], "Network");
}

// ---------- Branch pattern from the guidelines ----------

fn file_body(text: &str) -> serde_json::Value {
    use base64::Engine as _;
    json!({"type": "file", "encoding": "base64", "size": text.len(),
           "content": base64::engine::general_purpose::STANDARD.encode(text)})
}

async fn suggest(app: &Router, repo: &str) -> (StatusCode, serde_json::Value) {
    let (status, body) = send_request(
        app,
        request("POST", "/api/github/branch-suggestion")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "repo": repo }).to_string()))
            .unwrap(),
    )
    .await;
    assert!(!body.contains(TOKEN), "token in the answer: {body}");
    (status, german_or_default(&body))
}

#[tokio::test]
async fn a_pattern_is_proposed_from_contributing_and_nothing_is_changed() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/web/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_body(
            "# Contributing\nName your branch feature/PROJ-123-short-title or fix/PROJ-9-x.",
        )))
        .mount(&gh)
        .await;
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let (status, a) = suggest(&app, "firma/web").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(a["pattern"], "{type}/{ticket}-{slug}");
    assert_eq!(a["source"], "CONTRIBUTING.md");
    assert!(!a["message"].as_str().unwrap().is_empty());
    let requests = gh.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn without_a_finding_the_answer_is_a_friendly_hint() {
    // No file at all.
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .mount(&gh)
        .await;
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let (status, a) = suggest(&app, "firma/web").await;
    assert_eq!(status, StatusCode::OK);
    assert!(a["pattern"].is_null());
    assert!(
        a["message"]
            .as_str()
            .unwrap()
            .contains("keine Datei CONTRIBUTING.md")
    );

    // A file that says nothing about branches.
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/web/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_body("Be kind.")))
        .mount(&gh)
        .await;
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let (status, a) = suggest(&app, "firma/web").await;
    assert_eq!(status, StatusCode::OK);
    assert!(a["pattern"].is_null());
    assert_eq!(a["source"], "CONTRIBUTING.md");
    assert!(a["message"].as_str().unwrap().contains("selbst ein"));
}

#[tokio::test]
async fn the_proposal_needs_a_connection_and_a_real_repo_name() {
    let gh = MockServer::start().await;
    let app = gh_app(&gh.uri(), None, None);
    let (status, a) = suggest(&app, "firma/web").await;
    assert_eq!(status, StatusCode::CONFLICT, "{a}");
    let app = gh_app(&gh.uri(), Some(TOKEN), None);
    let (status, _) = suggest(&app, "../etc/passwd").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(gh.received_requests().await.unwrap().is_empty());
}

// ---- GitHub Projects ----

async fn projects(app: &Router) -> serde_json::Value {
    let (status, body) = send_request(
        app,
        request("GET", "/api/github/projects")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(TOKEN), "token in the answer: {body}");
    crate::german::german_json(&body)
}

#[tokio::test]
async fn without_a_token_the_project_list_says_not_connected() {
    let gh = MockServer::start().await;
    let v = projects(&gh_app(&gh.uri(), None, None)).await;
    assert_eq!(v["connected"], false);
    assert!(gh.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_project_list_shows_projects_and_what_a_classic_token_lacks() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "repo, read:project")
                .set_body_json(json!({"login": "mkeller"})),
        )
        .mount(&gh)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "viewer": {"login": "mkeller", "projectsV2": {"nodes": [
                {"id": "PVT_1", "number": 1, "title": "My board", "closed": false,
                 "url": "https://github.com/users/mkeller/projects/1",
                 "owner": {"__typename": "User", "login": "mkeller"}}]},
              "organizations": {"nodes": []}},
            "rateLimit": {"cost": 1, "remaining": 4999, "resetAt": "2999-01-01T00:00:00Z"}}})))
        .mount(&gh)
        .await;
    let v = projects(&gh_app(&gh.uri(), Some(TOKEN), None)).await;
    assert_eq!(v["connected"], true);
    assert_eq!(v["projects"][0]["title"], "My board");
    assert_eq!(v["owners"], json!(["mkeller"]));
    assert_eq!(v["rights"]["kind"], "classic");
    // Reading is allowed; changing projects is not (needs `project`), and the answer names the fix.
    let missing: Vec<String> = v["rights"]["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["right"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(missing, ["projects_write"]);
    assert!(v["problem"].is_null());
}

#[tokio::test]
async fn a_refused_project_list_names_the_missing_right_for_a_fine_grained_token() {
    let gh = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "mkeller"})))
        .mount(&gh)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"viewer": null},
            "errors": [{"type": "FORBIDDEN", "path": ["viewer", "projectsV2"],
                        "message": "Resource not accessible by personal access token"}]})))
        .mount(&gh)
        .await;
    let v = projects(&gh_app(&gh.uri(), Some("github_pat_11AAA"), None)).await;
    assert_eq!(v["rights"]["kind"], "fine_grained");
    let problem = v["problem"].as_str().unwrap_or_default();
    assert!(problem.contains("fehlt ein Recht"), "{problem}");
    assert!(v["projects"].as_array().unwrap().is_empty());
}
