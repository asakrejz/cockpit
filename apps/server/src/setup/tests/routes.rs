use super::*;

#[tokio::test]
async fn key_lands_in_env_file_and_comes_never_back() {
    let p = test_bench(None);
    let (status, body) = put(
        &p.app,
        "/api/secrets/github-oauth",
        r#"{"value":"  test-wert-123 "}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("test-wert-123"), "{body}");
    assert!(body.contains(r#""set":true"#));
    assert_eq!(
        p.secrets.get("github-oauth").unwrap().unwrap().expose(),
        "test-wert-123"
    );
    // Neither the setup nor the health report show the value.
    let e = get(&p.app, "/api/setup").await;
    assert!(!e.to_string().contains("test-wert-123"));
    let flag = |n: &str| {
        e["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == n)
            .unwrap()["set"]
            .as_bool()
            .unwrap()
    };
    assert!(flag("github-oauth") && !flag("atlassian-oauth"));
    let h = get(&p.app, "/api/health").await;
    assert!(!h.to_string().contains("test-wert-123"));
}

#[tokio::test]
async fn key_only_with_allowed_names_and_valid_value() {
    let p = test_bench(None);
    for name in ["github-oauth-token", "unsinn", "Jira"] {
        let (status, body) = put(
            &p.app,
            &format!("/api/secrets/{name}"),
            r#"{"value":"test-wert-123"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{name}");
        assert!(!body.contains("test-wert-123"));
    }
    assert!(p.secrets.get("github-oauth-token").unwrap().is_none());
    for (body, expected) in [
        (r#"{"value":""}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (r#"{"value":"  "}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (
            r#"{"value":"test-wert\n123"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("kein json test-wert-123", StatusCode::BAD_REQUEST),
        (r#"{"anders":"test-wert-123"}"#, StatusCode::BAD_REQUEST),
    ] {
        let (status, answer) = put(&p.app, "/api/secrets/litellm", body).await;
        assert_eq!(status, expected, "{body}");
        assert!(!answer.contains("test-wert"), "{answer}");
    }
    assert!(p.secrets.get("litellm").unwrap().is_none());
}

#[tokio::test]
async fn new_routes_have_same_checks_like_all_api_routes() {
    let p = test_bench(None);
    let value = r#"{"value":"test-wert-123"}"#;
    // Content-Type muss JSON sein.
    for kind in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut b = request("PUT", "/api/secrets/litellm");
        if let Some(t) = kind {
            b = b.header("content-type", t);
        }
        let (status, _) = send_request(&p.app, b.body(Body::from(value)).unwrap()).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    // Foreign origin, foreign host, foreign peer.
    let (status, _) = send_request(
        &p.app,
        request("PUT", "/api/secrets/litellm")
            .header("content-type", "application/json")
            .header("origin", "https://boese.example")
            .body(Body::from(value))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_request(
        &p.app,
        Request::builder()
            .method("PUT")
            .uri("/api/secrets/litellm")
            .header("host", "boese.example")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::from(value))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_request(
        &p.app,
        Request::builder()
            .method("PUT")
            .uri("/api/config")
            .header("host", "127.0.0.1:8787")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))))
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send_request(
        &p.app,
        Request::builder()
            .uri("/api/setup")
            .header("host", "boese.example")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(p.secrets.get("litellm").unwrap().is_none());
    assert!(!p.path.exists());
}

#[tokio::test]
async fn the_agent_key_goes_in_the_env_file_and_never_back() {
    let p = test_bench(None);
    let (status, answer) = put(
        &p.app,
        "/api/secrets/agent-key",
        r#"{"value":"sk-test-123"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(!answer.contains("sk-test-123"), "{answer}");
    assert!(p.secrets.get("agent-key").unwrap().is_some());
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpoint"]["key"], true);
    assert!(!h.to_string().contains("sk-test-123"));
    let e = get(&p.app, "/api/setup").await;
    assert!(!e.to_string().contains("sk-test-123"));
}

#[tokio::test]
async fn config_writes_the_file_and_takes_effect_without_restart() {
    let p = test_bench(Some(WITH_COMMENTS));
    // Before: nothing set up.
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpoint"]["configured"], false);

    let body = r#"{"endpoint":{"provider":"claude-agent-sdk","url":"localhost:4000","model":"claude-sonnet-5-5"},
        "github_oauth":{"client_id":"Iv23li"},
        "github":{"repos":["firma/app","https://github.com/firma/web.git"]}}"#;
    let (status, answer) = put(&p.app, "/api/config", body).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let a: ConfigResponse = serde_json::from_str(&answer).unwrap();
    assert!(a.stored && a.takes_effect_immediately);

    // File: new, but comments and old values stay.
    let file = std::fs::read_to_string(&p.path).unwrap();
    assert!(file.contains("# Meine Einstellungen") && file.contains("port = 8787 # Standard"));
    assert!(
        file.contains("firma/web") && !file.contains("alt/repo"),
        "{file}"
    );
    assert!(Config::parse(Some(&file), |_| None).is_ok());

    // Visible without restart.
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpoint"]["url"], "http://localhost:4000");
    assert_eq!(h["endpoint"]["model"], "claude-sonnet-5-5");
    // The older names of the request (`litellm`) still apply too.
    let (status, answer) = put(
        &p.app,
        "/api/config",
        r#"{"litellm":{"url":"localhost:4001"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let e = get(&p.app, "/api/setup").await;
    assert_eq!(e["endpoint_url"], "http://localhost:4001");
    assert_eq!(e["endpoint_model"], "claude-sonnet-5-5");
    assert_eq!(e["provider"], "claude-agent-sdk");
    let e = get(&p.app, "/api/setup").await;
    assert_eq!(e["github_client_id"], "Iv23li");
    assert_eq!(
        e["github_repos"],
        serde_json::json!(["firma/app", "firma/web"])
    );
    assert_eq!(
        e["callback_github"],
        "http://127.0.0.1:8787/api/oauth/github/callback"
    );
    assert_eq!(
        e["callback_atlassian"],
        "http://localhost:8787/api/oauth/atlassian/callback"
    );
    // With client ID and secret "Mit GitHub anmelden" is possible.
    put(
        &p.app,
        "/api/secrets/github-oauth",
        r#"{"value":"test-wert-123"}"#,
    )
    .await;
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["sign_in"]["github"]["oauth_possible"], true);
}

#[tokio::test]
async fn invalid_inputs_change_nothing_and_say_why() {
    let p = test_bench(Some(WITH_COMMENTS));
    let (status, body) = put(
        &p.app,
        "/api/config",
        r#"{"github":{"repos":["kein-repo"]}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains("kein-repo") && body.contains("firma/app"),
        "{body}"
    );
    assert_eq!(std::fs::read_to_string(&p.path).unwrap(), WITH_COMMENTS);
    let (status, _) = put(&p.app, "/api/config", "kein json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn new_client_id_brings_the_hint_to_new_sign_in() {
    let p = test_bench(None);
    let id = |i: &str| format!(r#"{{"atlassian":{{"client_id":"{i}"}}}}"#);
    let (_, first) = put(&p.app, "/api/config", &id("alt")).await;
    assert!(
        serde_json::from_str::<ConfigResponse>(&first)
            .unwrap()
            .hint
            .is_none()
    );
    let (_, second) = put(&p.app, "/api/config", &id("neu")).await;
    assert!(
        serde_json::from_str::<ConfigResponse>(&second)
            .unwrap()
            .hint
            .is_some()
    );
}

#[tokio::test]
async fn without_file_is_it_created() {
    let p = test_bench(None);
    let (status, _) = put(
        &p.app,
        "/api/config",
        r#"{"jira":{"site":"firma","email":"ich@firma.example"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        std::fs::read_to_string(&p.path)
            .unwrap()
            .contains("https://firma.atlassian.net")
    );
    // Jira via token still needs the key: only after that "connected".
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["sign_in"]["jira"]["connected"], false);
    put(&p.app, "/api/secrets/jira", r#"{"value":"test-wert-123"}"#).await;
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["sign_in"]["jira"]["connected"], true);
}
