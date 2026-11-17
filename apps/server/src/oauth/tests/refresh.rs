use super::*;

fn expired(p: &TestBench, d: Service, refresh: Option<&str>) {
    let t = Tokens {
        access: Secret::new("alt_zugriff"),
        refresh: refresh.map(Secret::new),
        expires_in_s: Some(1),
    };
    p.state.oauth_tokens_set(d, &t).unwrap();
}

#[tokio::test]
async fn expired_tokens_are_refreshed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "neu_zugriff", "expires_in": 28800, "refresh_token": "neu_erneuern"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_refresh().await;
    let t = p.state.oauth_tokens(Service::GitHub).unwrap();
    assert_eq!(t.access.expose(), "neu_zugriff");
    assert_eq!(t.refresh.unwrap().expose(), "neu_erneuern");
    // Still valid: no second call (`expect(1)`).
    p.state.oauth_refresh().await;
    // The client gets the new token.
    let d = p.state.services_build();
    assert!(d.github.is_some());
}

#[tokio::test]
async fn fails_the_refresh_is_the_service_disconnected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_refresh_token", "error_description": "abgelaufen"
        })))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_refresh().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_none());
    let s = p.state.service_status(Service::GitHub, false);
    assert!(!s.connected);
    assert_eq!(s.kind, AuthKind::None);

    // Without a refresh token the expired access cannot be saved.
    expired(&p, Service::GitHub, None);
    p.state.oauth_refresh().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_none());
}

#[tokio::test]
async fn outage_the_service_disconnects_not() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_refresh().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_some());
}
