use super::*;

#[test]
fn labels_and_repo_come_from_the_service() {
    let mut local = ticket("X-1", "Titel");
    local.labels = vec!["alt".into()];
    let mut remote = ticket("X-1", "Titel");
    remote.labels = vec!["neu".into(), "bug".into()];
    let merged = merge_item(local, &remote, &mut Vec::new());
    assert_eq!(merged.labels, ["neu", "bug"], "the service wins");

    // If the service removes all labels, they disappear here too.
    let mut local = ticket("X-1", "Titel");
    local.labels = vec!["alt".into()];
    let merged = merge_item(local, &ticket("X-1", "Titel"), &mut Vec::new());
    assert!(merged.labels.is_empty());

    let mut pr = pr_item(7);
    pr.repo = Some("a/x".into());
    let merged = merge_item(pr_item(7), &pr, &mut Vec::new());
    assert_eq!(merged.repo.as_deref(), Some("a/x"));
}

#[test]
fn the_same_pr_number_in_two_repos_stays_two_entries() {
    let mut a = pr_item(7);
    a.id = "gh-a-x-7".into();
    a.repo = Some("a/x".into());
    let mut b = pr_item(7);
    b.id = "gh-a-y-7".into();
    b.repo = Some("a/y".into());
    let ws = synced(&github(vec![a.clone(), b.clone()], Vec::new()));
    assert_eq!(ws.items.len(), 2);
    // A second sync changes nothing about it, and each repo keeps its own.
    let (ws, _) = merge(&ws, &github(vec![b, a], Vec::new()), T + 1);
    assert_eq!(ws.items.len(), 2);
    let repos: Vec<_> = ws.items.iter().filter_map(|i| i.repo.as_deref()).collect();
    assert!(repos.contains(&"a/x") && repos.contains(&"a/y"));
}

#[test]
fn old_saves_without_labels_and_repo_still_load() {
    let mut v = serde_json::to_value(ticket("X-1", "T")).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("labels");
    o.remove("repo");
    let item: Item = serde_json::from_value(v).unwrap();
    assert!(item.labels.is_empty() && item.repo.is_none());
}

#[test]
fn failed_source_keeps_its_data_and_reports_the_problem() {
    let ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
    let failed = |p: Problem| Remote {
        sources: vec![SourceData {
            source: Source::Jira,
            outcome: Outcome::Failed(p),
        }],
    };
    let (ws, changes) = merge(&ws, &failed(Problem::Unauthorized), T + 5);
    assert!(ws.item("abc-1").is_some(), "nothing is lost");
    let state = ws.sync.state_of(Source::Jira).unwrap();
    assert_eq!(state.problem, Some(Problem::Unauthorized));
    assert_eq!(state.id(), "unauthorized");
    assert_eq!(state.last_ok_ms, Some(T));
    assert_eq!(state.last_try_ms, T + 5);
    assert_eq!(changes.len(), 1);
    // The same problem again is not another notification.
    let (ws, changes) = merge(&ws, &failed(Problem::Unauthorized), T + 6);
    assert!(changes.is_empty());
    // Quota: pause until the point in time.
    let (ws, _) = merge(
        &ws,
        &failed(Problem::RateLimited {
            until_ms: Some(T + 60_000),
        }),
        T + 7,
    );
    assert_eq!(ws.sync.paused_until(Source::Jira, T + 8), Some(T + 60_000));
    assert_eq!(ws.sync.paused_until(Source::Jira, T + 60_001), None);
    assert_eq!(ws.sync.paused_until(Source::GitHub, T), None);
    // Recovered.
    let (ws, changes) = merge(&ws, &jira(vec![ticket("ABC-1", "Eins")]), T + 70_000);
    assert!(ws.sync.state_of(Source::Jira).unwrap().is_ok());
    assert_eq!(
        changes,
        vec![Change::SourceRecovered {
            source: Source::Jira
        }]
    );
}

#[test]
fn drafts_follow_their_line_when_the_diff_shifts() {
    let old = [
        (1, LineKind::Context, "fn a() {"),
        (2, LineKind::Add, "    foo();"),
        (3, LineKind::Context, "}"),
    ];
    let mut ws = synced(&github(vec![pr_item(5)], vec![review(5, files(&old))]));
    let id = ws.add_line_comment(5, "a.rs", 1, "Warum?", None).unwrap();
    // A new commit inserts a line at the front.
    let new = [
        (1, LineKind::Add, "use x;"),
        (2, LineKind::Context, "fn a() {"),
        (3, LineKind::Add, "    foo();"),
        (4, LineKind::Context, "}"),
    ];
    let (after, changes) = merge(
        &ws,
        &github(vec![pr_item(5)], vec![review(5, files(&new))]),
        T + 1,
    );
    let c = &after.review(5).unwrap().line_comments[0];
    assert_eq!((c.id.as_str(), c.line), (id.as_str(), 2));
    assert!(changes.is_empty());

    // The line no longer exists: draft dropped, the text is in the log.
    let gone = [(1, LineKind::Context, "fn b() {")];
    let (after, changes) = merge(
        &after,
        &github(vec![pr_item(5)], vec![review(5, files(&gone))]),
        T + 2,
    );
    assert!(after.review(5).unwrap().line_comments.is_empty());
    assert!(changes[0].text().contains("Warum?"), "{changes:?}");
}

#[test]
fn a_review_number_from_the_demo_or_simulation_does_not_leak_into_real_state() {
    let mut ws = Workspace::default();
    ws.reviews.push(review(5, vec![]));
    ws.reviews[0].verdict = Verdict::Approve;
    ws.reviews[0].sent = true;
    let (after, _) = merge(&ws, &github(vec![pr_item(5)], vec![review(5, vec![])]), T);
    assert_eq!(after.review(5).unwrap().verdict, Verdict::default());
    assert!(!after.review(5).unwrap().sent);
    assert_eq!(after.sync.pr(5).unwrap().repo, "a/x");
}

#[test]
fn many_new_entries_become_one_log_line() {
    let many: Vec<Item> = (1..=8).map(|n| ticket(&format!("ABC-{n}"), "t")).collect();
    let (_, changes) = merge(&Workspace::default(), &jira(many), T);
    assert_eq!(
        changes,
        vec![Change::Summary(cockpit_i18n::msg!(
            CoreSyncSummaryAdded,
            n = 8
        ))]
    );
}

#[test]
fn only_the_system_may_apply_a_sync() {
    let mut ws = Workspace::default();
    let action = Action::SyncRemote {
        remote: Remote::default(),
    };
    assert_eq!(
        ws.apply(action.clone(), Actor::User, T),
        Err(crate::ActionError::SystemOnly)
    );
    assert!(ws.apply(action, Actor::System, T).is_ok());
}

#[test]
fn checks_of_github_pull_requests_are_not_simulated() {
    let mut r = review(5, vec![]);
    r.check_runs = vec![crate::CheckRun::new("Tests", Checks::Failed, 10, "")];
    let mut ws = synced(&github(vec![pr_item(5)], vec![r]));
    assert!(ws.restart_check(5, "Tests"));
    assert_eq!(ws.running_checks(), 0, "GitHub runs the check");
    assert!(ws.advance_checks().is_empty());
    assert_eq!(ws.review(5).unwrap().check_runs[0].status, Checks::Running);
}

#[test]
fn an_old_state_without_sync_still_loads() {
    let text = persist::save(&demo::workspace());
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["workspace"].as_object_mut().unwrap().remove("sync");
    let (ws, restored) = persist::restore(Some(&json.to_string()));
    assert!(restored);
    assert_eq!(ws.sync, SyncState::default());
}

#[test]
fn the_saved_form_has_no_secrets() {
    let ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
    let text = persist::save(&ws).to_lowercase();
    for word in ["token", "secret", "password", "authorization"] {
        assert!(!text.contains(word), "{word}");
    }
}
