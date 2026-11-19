use super::*;

// ---------- Pull request ----------

/// A finished real run of BAU-142: branch pushed to the local remote.
async fn done_world(github: &MockServer) -> World {
    let w = pr_welt(github);
    w.state.inner.live.set_state_for_test(demo::workspace());
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    w
}

async fn none_open(github: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(github)
        .await;
}

#[tokio::test]
async fn preview_shows_title_text_and_creates_nothing() {
    let github = MockServer::start().await;
    none_open(&github).await;
    let w = done_world(&github).await;
    let p: PullRequestPreview = w.state.pull_request_preview("BAU-142", "").await.unwrap();
    assert!(p.title.starts_with("BAU-142: "));
    assert!(p.body.contains("Fokus-Ring ergänzt."));
    assert_eq!((p.head.as_str(), p.base.as_str()), (BRANCH, "main"));
    assert_eq!(p.repo, "firma/app");
    assert_eq!(p.existing, None);
    let posts = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .count();
    assert_eq!(posts, 0, "the preview creates nothing");
    assert!(w.run().pull_request.is_none(), "and changes nothing");
}

#[tokio::test]
async fn the_pull_request_is_created_only_on_the_action_and_with_the_edited_text() {
    let github = MockServer::start().await;
    none_open(&github).await;
    Mock::given(method("POST"))
        .and(path("/repos/firma/app/pulls"))
        .and(wiremock::matchers::body_partial_json(json!({
            "title": "BAU-142: Mein Titel", "head": BRANCH, "base": "main", "body": "Text", "draft": true
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 31, "html_url": "https://github.example/firma/app/pull/31", "state": "open"
        })))
        .expect(1)
        .mount(&github)
        .await;
    let w = done_world(&github).await;
    // Finishing the run alone creates nothing.
    assert!(
        github
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "POST")
    );
    w.action(create("BAU-142: Mein Titel")).await.unwrap();
    let run = w.run();
    let pull = run.pull_request.clone().unwrap();
    assert_eq!(
        (pull.number, pull.existed),
        (31, false),
        "GitHub decides, not the browser"
    );
    assert_eq!(pull.url, "https://github.example/firma/app/pull/31");
    assert_eq!(run.pr_number, Some(31));
    let entry = w.ws().log.last().cloned().unwrap();
    assert_eq!(entry.actor, Actor::User);
    assert!(entry.text.contains("#31"));
    // Nothing else was called: no merge, no review request, no PATCH.
    let others: Vec<String> = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| !matches!(r.method.as_str(), "GET" | "POST"))
        .map(|r| format!("{} {}", r.method, r.url.path()))
        .collect();
    assert!(others.is_empty(), "{others:?}");
    // A second click does not create a duplicate.
    let e = w.action(create("BAU-142: Mein Titel")).await.unwrap_err();
    assert!(e.contains("#31"), "{e}");
}

#[tokio::test]
async fn an_existing_pull_request_is_linked_instead_of_duplicated() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"number": 12, "html_url": "https://github.example/firma/app/pull/12", "state": "open",
             "head": {"ref": BRANCH, "sha": "abc"}}
        ])))
        .mount(&github)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&github)
        .await;
    let w = done_world(&github).await;
    let p = w.state.pull_request_preview("BAU-142", "").await.unwrap();
    assert_eq!(p.existing.as_ref().map(|e| e.number), Some(12));
    w.action(create("BAU-142: x")).await.unwrap();
    let pull = w.run().pull_request.unwrap();
    assert_eq!((pull.number, pull.existed), (12, true));
    assert!(w.ws().log.last().unwrap().text.contains("schon"));
}

#[tokio::test]
async fn a_pull_request_created_in_the_meantime_is_found_after_the_422() {
    let github = MockServer::start().await;
    // First look-up: nothing; after the failed creation: theirs.
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .up_to_n_times(1)
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"number": 40, "html_url": "u", "state": "open", "head": {"ref": BRANCH, "sha": "a"}}
        ])))
        .mount(&github)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Validation Failed",
            "errors": [{"message": "A pull request already exists for firma:claude/x"}]
        })))
        .mount(&github)
        .await;
    let w = done_world(&github).await;
    // The preview uses up the first (empty) answer.
    let _ = w.state.pull_request_preview("BAU-142", "").await.unwrap();
    w.action(create("BAU-142: x")).await.unwrap();
    assert_eq!(w.run().pull_request.unwrap().number, 40);
}

#[tokio::test]
async fn errors_of_github_are_told_in_plain_words_and_change_nothing() {
    for (status, body, needle) in [
        (401, json!({"message": "Bad credentials"}), "401"),
        (403, json!({"message": "Resource not accessible"}), "403"),
        (
            422,
            json!({"message": "Validation Failed", "errors": [{"message": "No commits between main and x"}]}),
            "No commits between",
        ),
    ] {
        let github = MockServer::start().await;
        none_open(&github).await;
        Mock::given(method("POST"))
            .and(path("/repos/firma/app/pulls"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&github)
            .await;
        let w = done_world(&github).await;
        let before = w.ws();
        let e = w.action(create("BAU-142: x")).await.unwrap_err();
        assert!(e.contains(needle) && e.contains("nichts geändert"), "{e}");
        assert!(!e.contains(GITHUB));
        assert_eq!(w.run().pull_request, None);
        assert_eq!(w.ws().log.len(), before.log.len());
    }
}

#[tokio::test]
async fn no_pull_request_before_the_run_is_done_or_with_an_empty_title() {
    let github = MockServer::start().await;
    none_open(&github).await;
    let w = pr_welt(&github);
    w.state.inner.live.set_state_for_test(demo::workspace());
    let e = w.action(create("BAU-142: x")).await.unwrap_err();
    assert!(!e.is_empty());
    let w = done_world(&github).await;
    let e = w.action(create("  ")).await.unwrap_err();
    assert!(e.contains("Eingabe"), "{e}");
    assert!(
        github
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "POST")
    );
}

#[tokio::test]
async fn the_simulation_refuses_followups_and_pull_requests() {
    let w = welt(true);
    let mut settings = w.state.inner.settings.clone();
    settings.seed = Seed::Demo;
    let conn = crate::db::open(&w.tmp.path().join("sim.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        settings,
    );
    state.inner.live.set_state_for_test(demo::workspace());
    for a in [
        Action::AddressReviewThreads {
            review: 85.into(),
            options: direct(),
        },
        create("x"),
    ] {
        let e = state.action_run(a).await;
        assert!(e.is_err());
    }
    let _ = FollowupTask::ReviewThreads;
}
