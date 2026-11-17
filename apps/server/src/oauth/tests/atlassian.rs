use super::*;

#[tokio::test]
async fn atlassian_a_site_is_automatic_chosen() {
    let server = MockServer::start().await;
    atlassian_mocks(
        &server,
        serde_json::json!([{"id":"cloud-1","name":"firma","url":"https://firma.atlassian.net","scopes":[]}]),
    )
    .await;
    let p = snapshot(&server, None, true);
    let ort = atlassian_sign_in(&p).await;
    assert!(
        ort.starts_with("http://127.0.0.1:8787/?oauth=ok&service=atlassian"),
        "{ort}"
    );
    assert!(param(&ort, "message").contains("firma"));
    assert_eq!(
        p.state.atlassian_site(),
        Some(("cloud-1".into(), "firma".into()))
    );
    let h = call(&p.app, "GET", "/api/health").await.2;
    let h: serde_json::Value = serde_json::from_str(&h).unwrap();
    assert_eq!(h["sign_in"]["jira"]["kind"], "oauth");
    assert_eq!(h["sign_in"]["jira"]["site"], "firma");
    assert_eq!(h["connections"]["jira"], true);
    // Jira is addressed through the gateway.
    let services = tokio::task::spawn_blocking({
        let s = p.state.clone();
        move || s.services_build()
    })
    .await
    .unwrap();
    assert!(services.jira.is_some());
}

#[tokio::test]
async fn atlassian_several_sites_need_a_choice() {
    let server = MockServer::start().await;
    atlassian_mocks(
        &server,
        serde_json::json!([
            {"id":"c1","name":"firma","url":"https://firma.atlassian.net","scopes":[]},
            {"id":"c2","name":"privat","url":"https://privat.atlassian.net","scopes":[]}
        ]),
    )
    .await;
    let p = snapshot(&server, None, true);
    let ort = atlassian_sign_in(&p).await;
    assert!(ort.contains("oauth=error"), "{ort}");
    let message = param(&ort, "message");
    assert!(
        message.contains("firma") && message.contains("privat"),
        "{message}"
    );
    // No half state.
    assert!(
        p.secrets
            .get(secrets::ATLASSIAN_OAUTH_TOKENS)
            .unwrap()
            .is_none()
    );
    assert_eq!(p.state.atlassian_site(), None);

    let p = snapshot(&server, Some("privat"), true);
    let ort = atlassian_sign_in(&p).await;
    assert!(ort.contains("oauth=ok"), "{ort}");
    assert_eq!(p.state.atlassian_site().unwrap().0, "c2");
}
