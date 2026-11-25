use super::*;

// ---- assistant: secret, status, test ------------------------------------------------------

#[tokio::test]
async fn the_generated_secret_is_shown_once_and_never_again() {
    let f = fixture(true, false, Duration::from_millis(50), 8787).await;
    let h: Headers = vec![("content-type", "application/json".into())];
    let (s, first) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/github",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let secret = serde_json::from_str::<serde_json::Value>(&first).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(secret.len(), 64);
    // It is stored (and the signature check uses it) ...
    assert_eq!(
        f.secrets
            .get(crate::secrets::GITHUB_WEBHOOK)
            .unwrap()
            .unwrap()
            .expose(),
        secret
    );
    // ... but no other answer contains it.
    for (m, p) in [
        ("GET", "/api/setup"),
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
    ] {
        let (_, text) = call(&f.state, m, p, "127.0.0.1:8787", &h, vec![]).await;
        assert!(!text.contains(&secret), "{p}");
    }
    let (_, setup) = call(&f.state, "GET", "/api/setup", "127.0.0.1:8787", &h, vec![]).await;
    let setup: serde_json::Value = serde_json::from_str(&setup).unwrap();
    assert_eq!(setup["webhooks"]["github"]["secret_set"], true);
    assert_eq!(setup["webhooks"]["jira"]["secret_set"], false);
    assert_eq!(setup["webhooks"]["github"]["path"], "/api/webhooks/github");
    // Renewing gives a different one and replaces the old.
    let (_, second) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/github",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    assert_ne!(first, second);
    assert!(
        !f.secrets
            .get(crate::secrets::GITHUB_WEBHOOK)
            .unwrap()
            .unwrap()
            .expose()
            .eq(&secret)
    );
    // An unknown source, and a foreign origin, are refused.
    let (s, _) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/gitlab",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let mut evil = h.clone();
    evil.push(("origin", "https://evil.example".into()));
    let (s, _) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-secret/github",
        "127.0.0.1:8787",
        &evil,
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

/// Starts the real router on a free port so that the local test request has somewhere to go.
async fn serve(f: &Fixture) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(f.state.clone(), PathBuf::from("/x"));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    port
}

#[tokio::test]
async fn the_test_button_makes_a_signed_round_trip_over_loopback() {
    // Bind first and keep the listener: releasing a probed port and binding it again later lets
    // a test running in parallel take it in between.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let f = fixture(true, true, Duration::from_millis(50), port).await;
    let app = router(f.state.clone(), PathBuf::from("/x"));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    let h: Headers = vec![("content-type", "application/json".into())];
    for source in ["github", "jira"] {
        let (s, text) = call(
            &f.state,
            "POST",
            &format!("/api/setup/webhook-test/{source}"),
            &format!("127.0.0.1:{port}"),
            &h,
            vec![],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let v = crate::german::german_json(&text);
        assert_eq!(v["ok"], true, "{source}: {text}");
        assert!(!text.contains(GH_SECRET) && !text.contains(JIRA_SECRET));
        // A second test is not swallowed by the replay check.
        let (_, text) = call(
            &f.state,
            "POST",
            &format!("/api/setup/webhook-test/{source}"),
            &format!("127.0.0.1:{port}"),
            &h,
            vec![],
        )
        .await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap()["ok"],
            true
        );
    }
    let (_, event) = f
        .state
        .inner
        .webhooks
        .last_received(Source::GitHub)
        .unwrap();
    assert_eq!(event, TEST_EVENT);
    // A test does not trigger a sync and does not make the webhooks "regular".
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
    assert!(f.state.inner.webhooks.poll_due(
        Source::Jira,
        Instant::now(),
        Duration::from_secs(120)
    ));

    // A wrong secret in the store would be reported: simulate by changing it between the calls?
    // No: the test signs with the stored one. Without a secret it says so.
    f.secrets.delete(crate::secrets::GITHUB_WEBHOOK).unwrap();
    let (_, text) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-test/github",
        &format!("127.0.0.1:{port}"),
        &h,
        vec![],
    )
    .await;
    let v = crate::german::german_json(&text);
    assert_eq!(v["ok"], false);
    assert!(v["message"].as_str().unwrap().contains("Secret"));
}

#[tokio::test]
async fn the_test_button_explains_when_webhooks_are_off() {
    let f = fixture(false, true, Duration::from_millis(50), 8787).await;
    let _ = serve(&f).await;
    let h: Headers = vec![("content-type", "application/json".into())];
    let (_, text) = call(
        &f.state,
        "POST",
        "/api/setup/webhook-test/github",
        "127.0.0.1:8787",
        &h,
        vec![],
    )
    .await;
    let v = crate::german::german_json(&text);
    assert_eq!(v["ok"], false);
    assert!(v["message"].as_str().unwrap().contains("aktiviert"));
}
