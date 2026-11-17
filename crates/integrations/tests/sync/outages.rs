use super::*;

#[tokio::test]
async fn errors_become_the_source_state_and_nothing_is_lost() {
    let g = github_server().await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    let broken = |response: ResponseTemplate| async move {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(response)
            .mount(&s)
            .await;
        s
    };
    // 401: token expired.
    let s = broken(ResponseTemplate::new(401)).await;
    let (after, _) = sync(&services(Some(&s), None), &ws, NOW_MS + 1).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::Unauthorized)
    );
    assert!(after.item("gh-acme-baukasten-87").is_some(), "Stand bleibt");

    // Quota: pause until reset.
    let s = broken(
        ResponseTemplate::new(403)
            .insert_header("x-ratelimit-remaining", "0")
            .insert_header("retry-after", "90"),
    )
    .await;
    let (after, _) = sync(&services(Some(&s), None), &ws, NOW_MS + 2).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::RateLimited {
            until_ms: Some(NOW_MS + 2 + 90_000)
        })
    );
    assert_eq!(
        after.sync.paused_until(Source::GitHub, NOW_MS + 3),
        Some(NOW_MS + 2 + 90_000)
    );
    // Paused: the source is not even queried.
    let empty = services(Some(&s), None)
        .fetch(&after, NOW_MS + 3, &[Source::GitHub])
        .await;
    assert!(empty.sources.is_empty());

    // Network gone.
    let gone = Services {
        github: Some(GitHubService {
            client: GitHubClient::new("http://127.0.0.1:9", Secret::new("x")).unwrap(),
            repos: vec![REPO.into()],
        }),
        jira: None,
    };
    let (after, _) = sync(&gone, &ws, NOW_MS + 4).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::Offline)
    );
    assert!(after.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn partial_outage_server_error_and_recovery() {
    let g = github_server().await;
    let j = jira_server().await;
    let both = services(Some(&g), Some(&j));
    let (ws, _) = sync(&both, &Workspace::default(), NOW_MS).await;
    assert!(ws.sync.sources.iter().all(|s| s.is_ok()));
    let tickets = ws.items.len();

    // Jira answers 500, GitHub is fine: one source is broken, the other keeps delivering.
    let broken = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&broken)
        .await;
    let partial = services(Some(&g), Some(&broken));
    let (after, _) = sync(&partial, &ws, NOW_MS + 1_000).await;
    let jira = after.sync.state_of(Source::Jira).unwrap();
    assert!(matches!(jira.problem, Some(Problem::Other(ref m)) if m.contains("500")));
    assert_eq!(jira.since_ms, Some(NOW_MS + 1_000));
    assert_eq!(jira.last_ok_ms, Some(NOW_MS), "last good state is kept");
    assert!(after.sync.state_of(Source::GitHub).unwrap().is_ok());
    assert_eq!(
        after.items.len(),
        tickets,
        "nothing disappears on an outage"
    );

    // Still broken later: `since` stays.
    let (still, _) = sync(&partial, &after, NOW_MS + 9_000).await;
    assert_eq!(
        still.sync.state_of(Source::Jira).unwrap().since_ms,
        Some(NOW_MS + 1_000)
    );

    // 429 without any header: paused state without a time, source is still queried next time.
    let limited = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&limited)
        .await;
    let (after, _) = sync(&services(Some(&limited), None), &ws, NOW_MS + 2_000).await;
    assert_eq!(
        after.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::RateLimited { until_ms: None })
    );
    assert_eq!(
        after.sync.paused_until(Source::GitHub, NOW_MS + 3_000),
        None
    );

    // Recovery: next good fetch clears the problem and `since`.
    let (healed, _) = sync(&both, &still, NOW_MS + 20_000).await;
    let jira = healed.sync.state_of(Source::Jira).unwrap();
    assert!(jira.is_ok());
    assert_eq!(jira.since_ms, None);
    assert_eq!(jira.last_ok_ms, Some(NOW_MS + 20_000));
}

#[tokio::test]
async fn rate_limit_reset_header_sets_the_retry_time() {
    let g = github_server().await;
    let (ws, _) = sync(&services(Some(&g), None), &Workspace::default(), NOW_MS).await;
    // GitHub style: 403/429 with the epoch second of the reset in `x-ratelimit-reset`.
    // The service's clock is the real one; the header is an absolute epoch second.
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", (now_s + 120).to_string()),
        )
        .mount(&s)
        .await;
    let (after, _) = sync(&services(Some(&s), None), &ws, NOW_MS + 5_000).await;
    let until = after
        .sync
        .paused_until(Source::GitHub, NOW_MS + 5_000)
        .expect("paused");
    let wait_ms = until - (NOW_MS + 5_000);
    assert!((118_000..=122_000).contains(&wait_ms), "{wait_ms}");
}

#[tokio::test]
async fn unknown_repo_is_a_hint_not_an_outage() {
    let g = github_server().await;
    let d = Services {
        github: Some(GitHubService {
            client: GitHubClient::new(&g.uri(), Secret::new("x")).unwrap(),
            repos: vec!["gibt/es-nicht".into(), REPO.into()],
        }),
        jira: None,
    };
    Mock::given(method("GET"))
        .and(path("/repos/gibt/es-nicht/pulls"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .mount(&g)
        .await;
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let st = ws.sync.state_of(Source::GitHub).unwrap();
    assert!(st.is_ok());
    assert!(
        st.warnings[0].contains("gibt/es-nicht"),
        "{:?}",
        st.warnings
    );
    assert!(ws.item("gh-acme-baukasten-87").is_some());
}
