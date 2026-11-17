use super::*;

#[tokio::test]
async fn jira_test_ok_reads_only_and_knows_the_names() {
    let jira = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .and(header_exists("authorization"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"accountId":"acc-1","displayName":"Mira Keller"}"#),
        )
        .mount(&jira)
        .await;
    let (app, _) = jira_state(&jira.uri(), "mira@firma.example", true);
    let a = check(&app).await;
    assert_eq!(a["ok"], true);
    assert_eq!(a["cause"], "ok");
    assert_eq!(a["name"], "Mira Keller");
    assert!(a["message"].as_str().unwrap().contains("kennt dich"));
    // Exactly one reading call, nothing else.
    let requests = jira.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "GET");
    assert_eq!(requests[0].url.path(), "/rest/api/3/myself");
}

#[tokio::test]
async fn jira_test_says_in_plain_text_what_it_read() {
    for (answer, cause) in [
        (ResponseTemplate::new(401), "token"),
        (ResponseTemplate::new(403), "permission"),
        (ResponseTemplate::new(404), "address"),
        // A foreign page instead of Jira.
        (
            ResponseTemplate::new(200).set_body_string("<html>Anmelden</html>"),
            "address",
        ),
        (ResponseTemplate::new(500), "other"),
    ] {
        let jira = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/myself"))
            .respond_with(answer)
            .mount(&jira)
            .await;
        let (app, _) = jira_state(&jira.uri(), "mira@firma.example", true);
        let a = check(&app).await;
        assert_eq!(a["ok"], false);
        assert_eq!(a["cause"], cause, "{a}");
        assert!(!a["message"].as_str().unwrap().is_empty());
    }
}

#[tokio::test]
async fn jira_test_without_connection_and_without_details() {
    // Nothing listens on the port.
    let (app, _) = jira_state("http://127.0.0.1:1", "mira@firma.example", true);
    assert_eq!(check(&app).await["cause"], "reachable");
    // Without token, without email, without site: "details", without anything being called.
    for (site, email, token) in [
        ("http://127.0.0.1:1", "mira@firma.example", false),
        ("http://127.0.0.1:1", "", true),
        ("", "", true),
    ] {
        let (app, _) = jira_state(site, email, token);
        let a = check(&app).await;
        assert_eq!(a["cause"], "details", "{site} {email} {token}");
        assert_eq!(a["ok"], false);
    }
}

#[tokio::test]
async fn jira_test_gives_the_token_never_out_also_when_jira_it_echoes() {
    let jira = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string(format!("Bad {TOKEN}")))
        .mount(&jira)
        .await;
    let (app, _) = jira_state(&jira.uri(), "mira@firma.example", true);
    let a = check(&app).await;
    assert_eq!(a["ok"], false);
    // `check` already checks that the token is not in the answer.
    assert!(!a.to_string().contains(TOKEN));
}

#[tokio::test]
async fn disconnect_deletes_only_tokens() {
    let (app, secrets) = jira_state("", "", true);
    secrets.set("litellm", &Secret::new(TOKEN)).unwrap();
    let delete = |name: &str| {
        request("DELETE", &format!("/api/secrets/{name}"))
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap()
    };
    let (status, body) = send_request(&app, delete("jira")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""set":false"#) && !body.contains(TOKEN));
    assert!(secrets.get("jira").unwrap().is_none());
    // A second time is no error.
    assert_eq!(send_request(&app, delete("jira")).await.0, StatusCode::OK);
    // The model key and client secrets stay.
    for name in ["litellm", "atlassian-oauth", "unsinn"] {
        assert_eq!(
            send_request(&app, delete(name)).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert!(secrets.get("litellm").unwrap().is_some());
    // Without a JSON content type the shared check rejects.
    let raw = request("DELETE", "/api/secrets/jira")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send_request(&app, raw).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}
