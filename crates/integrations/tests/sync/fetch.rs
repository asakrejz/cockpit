use super::*;

#[tokio::test]
async fn fetches_pull_requests_tickets_comments_checks_and_diffs() {
    let g = github_server().await;
    let j = jira_server().await;
    let d = services(Some(&g), Some(&j));
    let (ws, _) = sync(&d, &cockpit_core::demo::workspace(), NOW_MS).await;

    // Sample data is gone, real entries are there.
    assert!(ws.item("bau142").is_none() && ws.item("pr87").is_none());
    let pr87 = ws.item("gh-acme-baukasten-87").expect("PR 87");
    assert_eq!(pr87.pr.as_ref().unwrap().number, 87);
    assert_eq!(pr87.comments.len(), 4, "conversation and line comments");
    let pr91 = ws.item("gh-acme-baukasten-91").expect("PR 91");
    assert!(
        pr91.pr.as_ref().unwrap().changes_requested,
        "Urteil von mkeller"
    );
    assert!(
        pr91.comments
            .iter()
            .any(|c| c.text.starts_with("Review: Änderungen angefordert")),
    );
    assert_eq!(
        pr91.pr.as_ref().unwrap().checks,
        cockpit_core::Checks::Failed
    );
    let jira = ws.item("bau-142").expect("BAU-142");
    assert_eq!(jira.source, Source::Jira);
    assert!(!jira.comments.is_empty());

    // Diff and single checks in the review, mapping number → repo.
    let review = ws.review(87).unwrap();
    assert!(!review.files.is_empty());
    assert!(!review.check_runs.is_empty());
    assert_eq!(ws.review(91).unwrap().check_runs.len(), 3);
    assert_eq!(ws.sync.pr(87).unwrap().repo, REPO);
    assert_eq!(ws.sync.pr(87).unwrap().head_sha, "aaa111");
    assert!(ws.sync.state_of(Source::GitHub).unwrap().is_ok());
    assert!(ws.sync.state_of(Source::Jira).unwrap().is_ok());
    // The log records the new entry with author System.
    assert!(ws.log.iter().all(|e| e.actor == Actor::System));
    assert!(!ws.log.is_empty());
}

#[tokio::test]
async fn local_state_stays_on_the_next_sync() {
    let g = github_server().await;
    let j = jira_server().await;
    let d = services(Some(&g), Some(&j));
    let (mut ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;

    // Local: done, snoozed, review draft, verdict, agent run.
    ws.mark_done("bau-142");
    ws.snooze("gh-acme-baukasten-91", None);
    let line = ws.review(87).unwrap().files[0]
        .lines
        .iter()
        .position(|l| l.kind == cockpit_core::LineKind::Add)
        .unwrap();
    let file = ws.review(87).unwrap().files[0].name.clone();
    ws.add_line_comment(87, &file, line, "Eigener Entwurf", Some("neu"))
        .unwrap();
    ws.set_verdict(87, Verdict::Approve);
    ws.reviews
        .iter_mut()
        .find(|r| r.number == 87)
        .unwrap()
        .suggestions
        .push(cockpit_core::Suggestion::new(
            "s1",
            "x",
            cockpit_core::Level::Note,
            "Hm",
        ));
    ws.decide(87, "s1", Some(Decision::Declined));
    ws.start_agent("BAU-142", cockpit_core::AgentOptions::default())
        .unwrap();

    let (after, _) = sync(&d, &ws, NOW_MS + 120_000).await;
    assert!(after.item("bau-142").unwrap().read);
    assert!(after.snooze_of("gh-acme-baukasten-91").is_some());
    let r = after.review(87).unwrap();
    assert_eq!(r.line_comments.len(), 1);
    assert_eq!(r.line_comments[0].text, "Eigener Entwurf");
    assert_eq!(r.verdict, Verdict::Approve);
    assert_eq!(r.suggestions[0].decision, Some(Decision::Declined));
    assert_eq!(after.runs.len(), 1);
}

#[tokio::test]
async fn unchanged_pr_does_not_reload_the_diff() {
    let g = github_server().await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let before = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/pulls/87/files"))
        .count();
    let (_, remote) = sync(&d, &ws, NOW_MS + 1).await;
    let after = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/pulls/87/files"))
        .count();
    assert_eq!((before, after), (1, 1), "gleicher Commit: kein neuer Abruf");
    // And the diff is still in the remote.
    let Outcome::Loaded(l) = &remote.sources[0].outcome else {
        panic!("geladen erwartet")
    };
    assert!(
        !l.reviews
            .iter()
            .find(|r| r.number == 87)
            .unwrap()
            .files
            .is_empty()
    );
}

#[tokio::test]
async fn remote_disappears() {
    let g = github_server().await;
    let d = services(Some(&g), None);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert!(ws.item("gh-acme-baukasten-91").is_some());
    // At the service only #87 is left.
    g.reset().await;
    let g2 = github_server().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false author:@me repo:{REPO}"),
        ))
        .respond_with(ok(r#"{"items":[]}"#))
        .with_priority(1)
        .mount(&g2)
        .await;
    let d2 = services(Some(&g2), None);
    let (after, _) = sync(&d2, &ws, NOW_MS + 1).await;
    assert!(after.item("gh-acme-baukasten-91").is_none());
    assert!(after.review(91).is_none() && after.sync.pr(91).is_none());
    assert!(after.item("gh-acme-baukasten-87").is_some());
}
