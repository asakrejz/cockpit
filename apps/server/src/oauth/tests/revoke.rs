use super::*;

#[tokio::test]
async fn revoke_github_calls_the_service_and_deletes() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await;
    Mock::given(method("DELETE"))
        .and(path("/applications/gh-id/token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    let state = github_state(&p).await;
    call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=c&state={state}"),
    )
    .await;
    assert!(p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());

    let (status, _, body) = call(&p.app, "POST", "/api/oauth/github/revoke").await;
    assert_eq!(status, StatusCode::OK);
    let a: RevokeResponse = serde_json::from_str(&body).unwrap();
    assert!(a.disconnected && a.at_service, "{body}");
    assert!(!p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());
    // The client secret stays (for the next sign-in).
    assert!(p.secrets.has(secrets::GITHUB_OAUTH).unwrap());
    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["github"]["connected"], false);
    assert_eq!(h["sign_in"]["github"]["kind"], "none");
}

#[tokio::test]
async fn revoke_disconnects_also_when_the_service_fails() {
    let server = MockServer::start().await;
    token_mock_github(&server, 1).await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let p = snapshot(&server, None, true);
    let state = github_state(&p).await;
    call(
        &p.app,
        "GET",
        &format!("/api/oauth/github/callback?code=c&state={state}"),
    )
    .await;
    let a = p.state.oauth_revoke(Service::GitHub).await.unwrap();
    assert!(a.disconnected && !a.at_service);
    assert!(a.hint.unwrap().contains("nicht bestätigt"));
    assert!(!p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());
}

#[tokio::test]
async fn revoke_atlassian_deletes_tokens_and_site() {
    let server = MockServer::start().await;
    atlassian_mocks(
        &server,
        serde_json::json!([{"id":"c1","name":"firma","url":"https://firma.atlassian.net","scopes":[]}]),
    )
    .await;
    let p = snapshot(&server, None, true);
    atlassian_sign_in(&p).await;
    let (status, _, body) = call(&p.app, "POST", "/api/oauth/atlassian/revoke").await;
    assert_eq!(status, StatusCode::OK);
    let a: RevokeResponse = serde_json::from_str(&body).unwrap();
    assert!(a.disconnected && !a.at_service);
    assert!(a.hint.unwrap().contains("id.atlassian.com"));
    assert!(!p.secrets.has(secrets::ATLASSIAN_OAUTH_TOKENS).unwrap());
    assert_eq!(p.state.atlassian_site(), None);
    // Revoking without sign-in is no error.
    assert!(
        p.state
            .oauth_revoke(Service::Atlassian)
            .await
            .unwrap()
            .disconnected
    );
}
