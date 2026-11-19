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

#[tokio::test]
async fn an_expired_sign_in_without_refresh_falls_back_to_the_token() {
    let server = MockServer::start().await;
    let p = snapshot(&server, None, true);
    expired(&p, Service::GitHub, None);
    // Only the sign-in, and it ran out: not connected, and it says why.
    let s = p.state.service_status(Service::GitHub, false);
    assert!((s.connected, s.expired, s.kind) == (false, true, AuthKind::None));
    // The token keeps GitHub connected, and the sync uses it, not the dead access.
    let s = p.state.service_status(Service::GitHub, true);
    assert!((s.connected, s.expired, s.kind) == (true, true, AuthKind::Token));
    assert!(p.state.github_credentials().is_none_or(|(_, app)| !app));
    // With a refresh token the sign-in still counts as working.
    expired(&p, Service::GitHub, Some("erneuern"));
    let s = p.state.service_status(Service::GitHub, false);
    assert!((s.connected, s.expired, s.kind) == (true, false, AuthKind::OAuth));
}

fn github_signed_in(p: &TestBench) {
    let t = Tokens {
        access: Secret::new("app_zugriff"),
        refresh: Some(Secret::new("erneuern")),
        expires_in_s: Some(28800),
    };
    p.state.oauth_tokens_set(Service::GitHub, &t).unwrap();
    p.secrets
        .set(secrets::GITHUB, &Secret::new("dummy-pat"))
        .unwrap();
}

fn choose(p: &TestBench, github: Option<cockpit_core::AuthChoice>) {
    let mut q = (*p.state.sources()).clone();
    q.github_auth = github;
    *p.state.inner.sources.write().unwrap() = Arc::new(q);
}

#[tokio::test]
async fn the_chosen_method_decides_which_github_access_is_used() {
    use cockpit_core::AuthChoice;
    let server = MockServer::start().await;
    let p = snapshot(&server, None, true);
    github_signed_in(&p);
    let used = |p: &TestBench| {
        p.state
            .github_credentials()
            .map(|(t, app)| (t.expose().to_string(), app))
    };
    // No choice: the sign-in first, as before.
    assert_eq!(used(&p), Some(("app_zugriff".into(), true)));
    // Token chosen: the token is used although a working sign-in exists.
    choose(&p, Some(AuthChoice::Token));
    assert_eq!(used(&p), Some(("dummy-pat".into(), false)));
    let s = p.state.service_status(Service::GitHub, true);
    assert_eq!((s.connected, s.kind), (true, AuthKind::Token));
    assert!(s.oauth_stored);
    // Sign-in chosen: back to the app access.
    choose(&p, Some(AuthChoice::OAuth));
    assert_eq!(used(&p), Some(("app_zugriff".into(), true)));
    // Sign-in chosen but gone: the token is not used as a stand-in.
    p.state.oauth_tokens_delete(Service::GitHub);
    assert_eq!(used(&p), None);
    assert!(!p.state.service_status(Service::GitHub, true).connected);
}
