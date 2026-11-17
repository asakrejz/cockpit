use super::*;

#[tokio::test]
async fn a_signed_event_triggers_a_sync_of_that_source_only() {
    let f = plain().await;
    assert_eq!(
        gh(&f, "pull_request", "d1", r#"{"action":"opened"}"#).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count(&f.github, "/user").await, 1);
    assert_eq!(
        count(&f.jira, "/rest/api/3/myself").await,
        0,
        "Jira must not be asked"
    );

    assert_eq!(
        jira(
            &f,
            r#"{"webhookEvent":"jira:issue_updated","issue":{"key":"WEB-1"}}"#
        )
        .await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.jira, "/rest/api/3/myself", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count(&f.github, "/user").await, 1);
    assert_eq!(count(&f.jira, "/rest/api/3/myself").await, 1);
}

#[tokio::test]
async fn the_payload_is_never_taken_over_as_data() {
    let f = plain().await;
    let body = r#"{"action":"opened","pull_request":{"title":"INJECTED-TITLE"}}"#;
    assert_eq!(
        gh(&f, "pull_request", "d1", body).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, snapshot) = call(
        &f.state,
        "GET",
        "/api/workspace",
        "127.0.0.1:8787",
        &vec![],
        vec![],
    )
    .await;
    assert!(!snapshot.contains("INJECTED-TITLE"));
}

#[tokio::test]
async fn rejects_bad_signatures_without_syncing() {
    let f = plain().await;
    let body = br#"{"action":"opened"}"#.to_vec();
    let good = gh_headers(&body, "pull_request", "d1");
    let post = |h: Headers, b: Vec<u8>| {
        let s = f.state.clone();
        async move { call(&s, "POST", GITHUB_PATH, "cockpit.example", &h, b).await }
    };
    // Missing.
    let mut h = good.clone();
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    assert_eq!(post(h, body.clone()).await.0, StatusCode::UNAUTHORIZED);
    // Wrong secret.
    let mut h = good.clone();
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    h.push(("x-hub-signature-256", signature_header(b"wrong", &body)));
    assert_eq!(post(h, body.clone()).await.0, StatusCode::UNAUTHORIZED);
    // Right signature, other body.
    let (status, text) = post(good.clone(), br#"{"action":"closed"}"#.to_vec()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(text.is_empty(), "no details to the outside");
    // Wrong length, not hex.
    for bad in ["sha256=abcd", "sha256=zz", "nonsense"] {
        let mut h = good.clone();
        h.retain(|(k, _)| *k != "x-hub-signature-256");
        h.push(("x-hub-signature-256", bad.into()));
        assert_eq!(post(h, body.clone()).await.0, StatusCode::UNAUTHORIZED);
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
    assert!(
        f.state
            .inner
            .webhooks
            .last_received(Source::GitHub)
            .is_none()
    );
}

#[tokio::test]
async fn upper_case_hex_signature_is_accepted() {
    let f = plain().await;
    let body = br#"{"action":"opened"}"#.to_vec();
    let mut h = gh_headers(&body, "pull_request", "d1");
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    let upper = signature_header(GH_SECRET.as_bytes(), &body)
        .trim_start_matches("sha256=")
        .to_uppercase();
    h.push(("x-hub-signature-256", format!("sha256={upper}")));
    let (status, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn off_means_not_found_even_with_a_valid_signature() {
    let f = fixture(false, true, Duration::from_millis(50), 8787).await;
    assert_eq!(
        gh(&f, "pull_request", "d1", "{}").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"jira:issue_updated"}"#).await,
        StatusCode::NOT_FOUND
    );
    let (s, _) = call(
        &f.state,
        "GET",
        GITHUB_PATH,
        "cockpit.example",
        &vec![],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
}

#[tokio::test]
async fn without_a_secret_nothing_is_accepted() {
    let f = fixture(true, false, Duration::from_millis(50), 8787).await;
    assert_eq!(
        gh(&f, "pull_request", "d1", "{}").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"jira:issue_updated"}"#).await,
        StatusCode::NOT_FOUND
    );
    // An empty secret counts as missing; the signature of an empty key must not pass.
    f.secrets
        .set(crate::secrets::GITHUB_WEBHOOK, &Secret::new(""))
        .unwrap();
    let body = b"{}".to_vec();
    let mut h = gh_headers(&body, "pull_request", "d1");
    h.retain(|(k, _)| *k != "x-hub-signature-256");
    h.push(("x-hub-signature-256", signature_header(b"", &body)));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_post_json_within_the_size_limit() {
    let f = plain().await;
    let (s, _) = call(
        &f.state,
        "GET",
        GITHUB_PATH,
        "cockpit.example",
        &vec![],
        vec![],
    )
    .await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
    // Not JSON (form encoding is what GitHub offers as the other option).
    let body = b"payload=%7B%7D".to_vec();
    let mut h = gh_headers(&body, "pull_request", "d1");
    h.retain(|(k, _)| *k != "content-type");
    h.push(("content-type", "application/x-www-form-urlencoded".into()));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // `application/json; charset=utf-8` is fine.
    let body = b"{}".to_vec();
    let mut h = gh_headers(&body, "ping", "d9");
    h.retain(|(k, _)| *k != "content-type");
    h.push(("content-type", "Application/JSON; charset=utf-8".into()));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::OK);
    // Too big: with and without a declared length.
    let big = vec![b'a'; MAX_BODY + 1];
    let h = gh_headers(&big, "pull_request", "d2");
    let (s, _) = call(
        &f.state,
        "POST",
        GITHUB_PATH,
        "cockpit.example",
        &h,
        big.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let mut h = h;
    h.push(("content-length", "10".into()));
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, big).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // Exactly at the limit still passes the size check (and fails on the signature).
    let edge = vec![b'a'; MAX_BODY];
    let mut h = gh_headers(b"x", "pull_request", "d3");
    h.retain(|(k, _)| *k != "content-length");
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, edge).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ping_is_answered_and_irrelevant_events_ignored() {
    let f = plain().await;
    assert_eq!(gh(&f, "ping", "p1", r#"{"zen":"x"}"#).await, StatusCode::OK);
    assert_eq!(
        gh(&f, "star", "s1", r#"{"a":1}"#).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"sprint_started"}"#).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(jira(&f, "not json").await, StatusCode::NO_CONTENT);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(count(&f.github, "/user").await, 0);
    assert_eq!(count(&f.jira, "/rest/api/3/myself").await, 0);
    // They are still shown as "received" (event type only) ...
    let (_, event) = f
        .state
        .inner
        .webhooks
        .last_received(Source::GitHub)
        .unwrap();
    assert_eq!(event, "star");
    // ... but they do not count as regular webhooks for the poll interval.
    assert!(f.state.inner.webhooks.poll_due(
        Source::GitHub,
        Instant::now(),
        Duration::from_secs(120)
    ));
}

#[tokio::test]
async fn a_missing_github_event_header_is_a_bad_request() {
    let f = plain().await;
    let body = b"{}".to_vec();
    let mut h = gh_headers(&body, "x", "d");
    h.retain(|(k, _)| *k != "x-github-event");
    let (s, _) = call(&f.state, "POST", GITHUB_PATH, "cockpit.example", &h, body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_replayed_delivery_is_ignored() {
    let f = plain().await;
    let body = r#"{"action":"opened"}"#;
    assert_eq!(
        gh(&f, "pull_request", "same-id", body).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    // The same ID again, and the same body under a new ID.
    assert_eq!(
        gh(&f, "pull_request", "same-id", r#"{"action":"closed"}"#).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        gh(&f, "pull_request", "other-id", body).await,
        StatusCode::ACCEPTED
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count(&f.github, "/user").await, 1);
    // A different delivery is a new one.
    assert_eq!(
        gh(&f, "pull_request", "third", r#"{"action":"edited"}"#).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 2).await;
}

#[tokio::test]
async fn bursts_are_merged_into_few_syncs() {
    let f = fixture(true, true, Duration::from_millis(400), 8787).await;
    assert_eq!(
        gh(&f, "pull_request", "b0", r#"{"n":0}"#).await,
        StatusCode::ACCEPTED
    );
    wait_for(&f.github, "/user", 1).await;
    // While the debounce time runs: many webhooks, one more sync.
    for i in 1..=10 {
        let body = format!(r#"{{"n":{i}}}"#);
        assert_eq!(
            gh(&f, "check_run", &format!("b{i}"), &body).await,
            StatusCode::ACCEPTED
        );
    }
    wait_for(&f.github, "/user", 2).await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(count(&f.github, "/user").await, 2);
}

#[tokio::test]
async fn rate_limit_answers_429() {
    let f = plain().await;
    let mut last = StatusCode::OK;
    for _ in 0..=RATE_LIMIT {
        let (s, _) = call(
            &f.state,
            "POST",
            GITHUB_PATH,
            "cockpit.example",
            &vec![("content-type", "application/json".into())],
            b"{}".to_vec(),
        )
        .await;
        last = s;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    // The other source has its own budget.
    assert_eq!(
        jira(&f, r#"{"webhookEvent":"cockpit:test"}"#).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn jira_accepts_the_signature_or_the_token() {
    let f = plain().await;
    let body = br#"{"webhookEvent":"jira:issue_created"}"#.to_vec();
    let json_type: Headers = vec![("content-type", "application/json".into())];
    // Token in the query.
    let uri = format!("{JIRA_PATH}?token={JIRA_SECRET}");
    let (s, _) = call(
        &f.state,
        "POST",
        &uri,
        "cockpit.example",
        &json_type,
        body.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    // Wrong or missing token.
    let body2 = br#"{"webhookEvent":"jira:issue_created","n":2}"#.to_vec();
    for uri in [
        format!("{JIRA_PATH}?token=nope"),
        format!("{JIRA_PATH}?token="),
        JIRA_PATH.to_string(),
    ] {
        let (s, _) = call(
            &f.state,
            "POST",
            &uri,
            "cockpit.example",
            &json_type,
            body2.clone(),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri}");
    }
    // A wrong signature is not rescued by a right token.
    let mut h = json_type.clone();
    h.push(("x-hub-signature", signature_header(b"wrong", &body2)));
    let uri = format!("{JIRA_PATH}?token={JIRA_SECRET}");
    let (s, _) = call(&f.state, "POST", &uri, "cockpit.example", &h, body2).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // GitHub never takes the token.
    let uri = format!("{GITHUB_PATH}?token={GH_SECRET}");
    let (s, _) = call(
        &f.state,
        "POST",
        &uri,
        "cockpit.example",
        &gh_headers(b"x", "ping", "z")
            .into_iter()
            .filter(|(k, _)| *k != "x-hub-signature-256")
            .collect(),
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[test]
fn query_token_and_event_names() {
    assert_eq!(query_token(Some("a=1&token=xyz")), Some("xyz"));
    assert_eq!(query_token(Some("token=")), None);
    assert_eq!(query_token(None), None);
    assert!(event_name_ok("jira:issue_updated"));
    assert!(!event_name_ok(""));
    assert!(!event_name_ok("a b"));
    assert!(!event_name_ok(&"a".repeat(65)));
    assert!(!event_name_ok("<script>"));
}
