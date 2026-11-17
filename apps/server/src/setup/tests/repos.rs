use super::*;

#[tokio::test]
async fn the_assistant_reads_the_agent_repos_back() {
    let b = test_bench(None);
    let body = serde_json::to_string(&agent_repos_change()).unwrap();
    let (status, answer) = put(&b.app, "/api/config", &body).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let setup = get(&b.app, "/api/setup").await;
    assert_eq!(setup["agent_repos"]["default"], "firma/app");
    assert_eq!(setup["agent_repos"]["repos"][1]["projects"][0], "WEB");
    // It takes effect without restart: the health report knows the routing.
    let health = get(&b.app, "/api/health").await;
    assert_eq!(
        health["agent"]["repos"]["repos"].as_array().unwrap().len(),
        2
    );
}

async fn repo_check(app: &Router, repos: &[&str]) -> serde_json::Value {
    let body = serde_json::json!({ "repos": repos }).to_string();
    let (status, answer) = send_request(
        app,
        request("POST", "/api/github/repos/check")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(!answer.contains(TOKEN), "token in the answer: {answer}");
    serde_json::from_str(&answer).unwrap()
}

#[tokio::test]
async fn the_repo_check_only_reads_and_says_what_it_found() {
    let gh = MockServer::start().await;
    for (repo, answer) in [
        (
            "firma/app",
            ResponseTemplate::new(200).set_body_string(
                r#"{"default_branch":"main","archived":false,"permissions":{"push":true,"pull":true}}"#,
            ),
        ),
        (
            "firma/lesen",
            ResponseTemplate::new(200).set_body_string(
                r#"{"default_branch":"trunk","permissions":{"push":false,"pull":true}}"#,
            ),
        ),
        ("firma/weg", ResponseTemplate::new(404)),
        ("firma/verboten", ResponseTemplate::new(401)),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/{repo}")))
            .respond_with(answer)
            .mount(&gh)
            .await;
    }
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
    let a = repo_check(
        &app,
        &[
            "firma/app",
            "firma/lesen",
            "firma/weg",
            "firma/verboten",
            "kein repo",
        ],
    )
    .await;
    let states: Vec<&str> = a["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        ["Ok", "ReadOnly", "NotFound", "NoAccess", "NotFound"]
    );
    assert_eq!(a["repos"][0]["default_branch"], "main");
    assert!(
        a["repos"][1]["message"]
            .as_str()
            .unwrap()
            .contains("Schreibrecht")
    );
    // Only GET requests, one per valid repo – nothing was changed anywhere.
    let requests = gh.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn the_repo_check_without_a_github_connection_says_so() {
    let (app, _) = jira_state("", "", false);
    let a = repo_check(&app, &["firma/app"]).await;
    assert_eq!(a["repos"][0]["state"], "NoAccess");
}
