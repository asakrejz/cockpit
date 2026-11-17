use super::*;

#[tokio::test]
async fn oauth_goes_before_the_api_token() {
    let server = MockServer::start().await;
    let p = snapshot(&server, None, true);
    p.secrets
        .set(secrets::GITHUB, &Secret::new("ghp_api_token_123"))
        .unwrap();
    let status = p.state.service_status(Service::GitHub, true);
    assert_eq!((status.connected, status.kind), (true, AuthKind::Token));
    p.state
        .oauth_tokens_set(
            Service::GitHub,
            &Tokens {
                access: Secret::new("ghu_x"),
                refresh: None,
                expires_in_s: None,
            },
        )
        .unwrap();
    let status = p.state.service_status(Service::GitHub, true);
    assert_eq!(status.kind, AuthKind::OAuth);
    assert!(status.oauth_possible);
    // After disconnecting the token applies again.
    p.state.oauth_revoke(Service::GitHub).await.unwrap();
    assert_eq!(
        p.state.service_status(Service::GitHub, true).kind,
        AuthKind::Token
    );
}

#[tokio::test]
async fn callback_is_a_navigation_but_only_from_this_machine() {
    let server = MockServer::start().await;
    let p = snapshot(&server, None, true);
    // Navigation of the browser: GET without JSON content type and without origin.
    let req = Request::builder()
        .uri("/api/oauth/github/callback?code=a&state=b")
        .header("host", "localhost:8787")
        .header("sec-fetch-mode", "navigate")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
        .body(Body::empty())
        .unwrap();
    let r = p.app.clone().oneshot(req).await.unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    // Foreign host (DNS rebinding) and foreign peer stay out.
    for (host, peer) in [
        ("evil.example:8787", [127, 0, 0, 1]),
        ("127.0.0.1:8787", [10, 0, 0, 5]),
    ] {
        let req = Request::builder()
            .uri("/api/oauth/github/callback?code=a&state=b")
            .header("host", host)
            .extension(ConnectInfo(SocketAddr::from((peer, 50000))))
            .body(Body::empty())
            .unwrap();
        let r = p.app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{host}");
    }
}

#[test]
fn addresses_and_encoding() {
    assert_eq!(
        redirect_uri(Service::GitHub, 8787),
        "http://127.0.0.1:8787/api/oauth/github/callback"
    );
    assert_eq!(
        redirect_uri(Service::Atlassian, 8787),
        "http://localhost:8787/api/oauth/atlassian/callback"
    );
    assert_eq!(encoded("Ä b&c=d#"), "%C3%84%20b%26c%3Dd%23");
}
