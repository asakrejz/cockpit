use super::*;

#[tokio::test]
async fn start_without_setup_shows_a_understandable_message() {
    let server = MockServer::start().await;
    // Without a client secret in the .env file.
    let p = snapshot(&server, None, false);
    let (status, location, _) = call(&p.app, "GET", "/api/oauth/github/start").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        location.starts_with("http://127.0.0.1:8787/?oauth=error&service=github"),
        "{location}"
    );
    let message = message(&location);
    assert!(
        message.contains("Schritt „GitHub“") && message.contains("Client-Secret"),
        "{message}"
    );

    // Without a client ID in the configuration.
    let p = TestBench {
        state: AppState::new(
            crate::db::open_in_memory().unwrap(),
            Arc::new(MemoryStore::new()),
        ),
        app: Router::new(),
        secrets: Arc::new(MemoryStore::new()),
    };
    let _ = &p.secrets;
    assert!(p.state.oauth_start(Service::Atlassian).await.is_err());

    // Unknown service.
    let p = snapshot(&server, None, true);
    let (status, _, _) = call(&p.app, "GET", "/api/oauth/gitlab/start").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn github_sign_in_from_start_until_health() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await;
    let p = snapshot(&server, None, true);
    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["github"]["connected"], false);
    assert_eq!(h["sign_in"]["github"]["oauth_possible"], true);

    let state = github_state(&p).await;
    let (status, location, body) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=abc&state={state}&installation_id=5&setup_action=install"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        location.starts_with("http://127.0.0.1:8787/?oauth=ok&service=github"),
        "{location}"
    );
    assert!(location.ends_with("#/setup"));
    // Nothing secret in redirect or body.
    for text in [&location, &body] {
        for secret in ["ghu_zugriff_geheim", "ghr_erneuern_geheim", CLIENT_SECRET] {
            assert!(!text.contains(secret));
        }
    }
    // Tokens are in the .env file.
    let stored = p
        .secrets
        .get(secrets::GITHUB_OAUTH_TOKENS)
        .unwrap()
        .unwrap();
    let t = Tokens::from_text(stored.expose()).unwrap();
    assert_eq!(t.access.expose(), "ghu_zugriff_geheim");
    assert_eq!(t.refresh.unwrap().expose(), "ghr_erneuern_geheim");

    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["github"]["connected"], true);
    assert_eq!(h["sign_in"]["github"]["kind"], "oauth");
    assert_eq!(h["connections"]["github"], true);
    assert!(!h.to_string().contains("geheim"));
}

#[tokio::test]
async fn wrong_and_duplicate_state() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await; // exactly one exchange
    let p = snapshot(&server, None, true);

    // Wrong state: no exchange, nothing stored.
    let _ = github_state(&p).await;
    let (_, location, _) = call(
        &p.app,
        "GET",
        "/api/oauth/github/callback?code=abc&state=falsch",
    )
    .await;
    assert!(location.contains("oauth=error"), "{location}");
    assert!(message(&location).contains("unbekannt"));
    // Without state (e.g. return from the installation without our start).
    let (_, location, _) = call(
        &p.app,
        "GET",
        "/api/oauth/github/callback?code=abc&installation_id=5&setup_action=install",
    )
    .await;
    assert!(location.contains("oauth=error"));
    assert!(message(&location).contains("Mit GitHub anmelden"));
    assert!(
        p.secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .is_none()
    );

    // Right, then the same callback once more.
    let state = github_state(&p).await;
    let url = format!("/api/oauth/github/callback?code=abc&state={state}");
    assert!(call(&p.app, "GET", &url).await.1.contains("oauth=ok"));
    let (_, location, _) = call(&p.app, "GET", &url).await;
    assert!(location.contains("oauth=error"), "{location}");
    assert!(message(&location).contains("schon verwendet"));
    assert!(
        p.secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .is_some()
    );

    // State from GitHub, callback from Atlassian.
    let state = github_state(&p).await;
    let (_, location, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/atlassian/callback?code=abc&state={state}"),
    )
    .await;
    assert!(message(&location).contains("anderen Dienst"), "{location}");
}

#[tokio::test]
async fn error_the_service_and_cancel() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_verification_code",
            "error_description": "Der Code ist abgelaufen"
        })))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    let state = github_state(&p).await;
    let (_, location, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=abc&state={state}"),
    )
    .await;
    assert!(location.contains("oauth=error"));
    assert!(message(&location).contains("abgelaufen"));
    assert!(
        p.secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .is_none()
    );

    // The user declines.
    let state = github_state(&p).await;
    let (_, location, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?error=access_denied&error_description=The+user+denied&state={state}"),
    )
    .await;
    assert!(
        message(&location).contains("abgebrochen oder abgelehnt: The user denied"),
        "{location}"
    );
    // The state is consumed by that.
    let (_, location, _) = call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=abc&state={state}"),
    )
    .await;
    assert!(message(&location).contains("schon verwendet"));
}
