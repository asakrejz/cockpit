use super::*;

// ---- the host exception is only these two routes ------------------------------------------

#[tokio::test]
async fn a_foreign_host_only_reaches_the_two_webhook_routes() {
    let f = plain().await;
    let json_type: Headers = vec![("content-type", "application/json".into())];
    let routes = [
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
        ("POST", "/api/actions"),
        ("POST", "/api/sync"),
        ("GET", "/api/events"),
        ("GET", "/api/setup"),
        ("PUT", "/api/config"),
        ("PUT", "/api/secrets/github"),
        ("DELETE", "/api/secrets/github"),
        ("POST", "/api/setup/webhook-secret/github"),
        ("POST", "/api/setup/webhook-test/github"),
        ("GET", "/api/oauth/github/start"),
        ("GET", "/api/agent/models"),
        ("GET", "/api/webhooks/other"),
        ("GET", "/api/webhooks"),
        ("POST", "/api/webhooks/github/extra"),
        ("GET", "/api/gibt-es-nicht"),
    ];
    for host in [
        "cockpit.example",
        "evil.example:8787",
        "127.0.0.1.evil.example",
        "127.0.0.1",
    ] {
        for (m, p) in routes {
            let (s, _) = call(&f.state, m, p, host, &json_type, b"{}".to_vec()).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{m} {p} with host {host}");
        }
    }
    // The same routes work with the real host (control).
    let (s, _) = call(
        &f.state,
        "GET",
        "/api/workspace",
        "127.0.0.1:8787",
        &vec![],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // The webhook routes work with a foreign host.
    assert_eq!(gh(&f, "ping", "x1", "{}").await, StatusCode::OK);
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"cockpit:test"}"#).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_remote_peer_is_refused_on_the_webhook_routes() {
    let f = plain().await;
    let body = b"{}".to_vec();
    let h = gh_headers(&body, "ping", "r1");
    let mut b = Request::builder()
        .method("POST")
        .uri(GITHUB_PATH)
        .header("host", "cockpit.example")
        .extension(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
    for (k, v) in &h {
        b = b.header(*k, v);
    }
    let app = router(f.state.clone(), PathBuf::from("/x"));
    let answer = app
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    assert_eq!(answer.status(), StatusCode::FORBIDDEN);
}
