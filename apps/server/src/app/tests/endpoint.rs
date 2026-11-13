use super::*;

#[tokio::test]
async fn check_reports_reachable_key_and_model_under_both_addresses() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "claude-sonnet-5-5"}]})),
        )
        .expect(1)
        .mount(&proxy)
        .await;
    // The key is under the older name `litellm`.
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::LITELLM, &Secret::new("sk-geheim-123456"))
        .unwrap();
    for path in ["/api/agent/check"] {
        let (status, body) = hole(
            with_endpoint(Some(&proxy.uri()), None, secrets.clone()),
            path,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("geheim"), "{body}");
        let j = json(&body);
        assert_eq!(j["configured"], true, "{path}");
        assert_eq!(j["reachable"], true);
        assert_eq!(j["key_ok"], true);
        assert_eq!(j["model_present"], true);
    }
}

#[tokio::test]
async fn check_without_key_calls_the_endpoint_not_on() {
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&proxy)
        .await;
    let (_, body) = hole(
        with_endpoint(Some(&proxy.uri()), None, Arc::new(MemoryStore::new())),
        "/api/agent/check",
    )
    .await;
    let j = json(&body);
    assert_eq!(j["configured"], true);
    assert_eq!(j["key_ok"], false);
    assert!(j["hint"].as_str().unwrap().contains("agent-key"));
}

#[tokio::test]
async fn check_without_setting_and_from_foreign_hosts() {
    let app = router(state(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    for path in ["/api/agent/check"] {
        let (status, body) = hole(app.clone(), path).await;
        assert_eq!(status, StatusCode::OK);
        let j = json(&body);
        assert_eq!(j["configured"], false);
        assert!(j["hint"].as_str().unwrap().contains("Modell"));
    }
    for path in ["/api/agent/models"] {
        let req = Request::builder()
            .uri(path)
            .header("host", "boese.example")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            send_request(app.clone(), req).await.0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn model_list_from_proxy_with_bearer_and_without_the_key() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"data": [{"id": "gpt-x"}, {"id": "claude-b"}, {"id": "claude-a"}]}),
        ))
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
    let (status, body) = hole(
        with_endpoint(Some(&proxy.uri()), None, secrets),
        "/api/agent/models",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("geheim"), "{body}");
    let j = json(&body);
    assert_eq!(j["loaded"], true);
    assert_eq!(
        j["models"],
        serde_json::json!(["claude-a", "claude-b", "gpt-x"])
    );
}

#[tokio::test]
async fn model_list_direct_takes_x_api_key() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let anthropic = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "sk-ant-geheim"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "claude-opus-5-5"}]})),
        )
        .expect(1)
        .mount(&anthropic)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-ant-geheim"))
        .unwrap();
    let (_, body) = hole(
        with_endpoint(None, Some(&anthropic.uri()), secrets),
        "/api/agent/models",
    )
    .await;
    assert!(!body.contains("geheim"), "{body}");
    assert_eq!(
        json(&body)["models"],
        serde_json::json!(["claude-opus-5-5"])
    );
}

#[tokio::test]
async fn model_list_reports_rejected_key_and_missing_key_in_plain_text() {
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-falsch-1"))
        .unwrap();
    let (_, body) = hole(
        with_endpoint(Some(&proxy.uri()), None, secrets),
        "/api/agent/models",
    )
    .await;
    let j = json(&body);
    assert_eq!(j["loaded"], false);
    assert_eq!(j["reachable"], true);
    assert!(j["hint"].as_str().unwrap().contains("lehnt den Key ab"));
    assert!(!body.contains("falsch"), "{body}");

    // Without a key nothing is called.
    let still = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&still)
        .await;
    let (_, body) = hole(
        with_endpoint(Some(&still.uri()), None, Arc::new(MemoryStore::new())),
        "/api/agent/models",
    )
    .await;
    assert!(json(&body)["hint"].as_str().unwrap().contains("kein Key"));
}

#[tokio::test]
async fn health_reports_the_agents_never_secrets() {
    // Simulated (tests start with the seed demo): always ready.
    let app = router(state(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["agent"]["mode"], "simulated");
    assert_eq!(j["agent"]["ready"], true);
    // Real, but nothing set up: says in plain text what is missing.
    let conn = db::open_in_memory().unwrap();
    let e = Settings {
        seed: Seed::Empty,
        ..empty()
    };
    let app = router(
        state_with(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["agent"]["mode"], "real");
    assert_eq!(j["agent"]["ready"], false);
    assert_eq!(j["agent"]["image_name"], "localhost/cockpit-agent:4");
    let missing = j["agent"]["missing"].as_array().unwrap();
    assert!(missing.len() >= 3);
    assert!(
        missing
            .iter()
            .all(|f| f.as_str().unwrap().contains("Einrichten"))
    );
}

#[tokio::test]
async fn image_build_without_runtime_is_422_with_plain_text() {
    let conn = db::open_in_memory().unwrap();
    let e = Settings {
        seed: Seed::Empty,
        ..empty()
    };
    let app = router(
        state_with(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (status, body) = hole(app.clone(), "/api/agent/image").await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&body);
    assert_eq!(j["running"], false);
    assert_eq!(j["present"], false);
    let req = request("POST", "/api/agent/image")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = send_request(app, req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["error"]
            .as_str()
            .unwrap()
            .contains("Rancher Desktop"),
        "{body}"
    );
}
