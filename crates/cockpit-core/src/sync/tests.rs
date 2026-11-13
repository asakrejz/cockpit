use super::*;
use crate::action::{Action, Actor};
use crate::model::{Comment, Kind, Person, PrRef, ReasonKind, Scope};
use crate::review::{Decision, LineKind, Suggestion, Verdict};
use crate::{notify, persist};

const T: u64 = 1_800_000_000_000;

fn ticket(key: &str, title: &str) -> Item {
    Item {
        id: key.to_lowercase(),
        kind: Kind::Ticket,
        source: Source::Jira,
        key: key.into(),
        title: title.into(),
        description: String::new(),
        reason: ReasonKind::Assigned,
        reason_text: "Dir zugewiesen".into(),
        time: "vor 5 Min.".into(),
        scope: Scope::Mine,
        owner: Person::me(),
        status: TicketStatus::Open,
        branch: None,
        pr: None,
        agent_note: None,
        comments: Vec::new(),
        read: false,
        due: None,
        priority: Default::default(),
        repo: None,
        labels: Vec::new(),
    }
}

fn pr_item(number: u32) -> Item {
    Item {
        id: format!("gh-a-x-{number}"),
        kind: Kind::PullRequest,
        source: Source::GitHub,
        key: format!("#{number}"),
        title: format!("PR {number}"),
        reason: ReasonKind::ReviewRequested,
        reason_text: "Review angefragt".into(),
        pr: Some(PrRef {
            repo: String::new(),
            number,
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: true,
        }),
        branch: Some("feature/x".into()),
        ..ticket("X-1", "")
    }
}

fn files(lines: &[(u32, LineKind, &str)]) -> Vec<FileDiff> {
    vec![FileDiff::new("a.rs", lines)]
}

fn review(number: u32, files: Vec<FileDiff>) -> Review {
    Review {
        repo: String::new(),
        number,
        title: format!("PR {number}"),
        description: String::new(),
        author: Person::new("Lena", "LK"),
        branch: "feature/x".into(),
        base: "main".into(),
        ticket_key: None,
        checks: Checks::Passed,
        files,
        agent_reviewed: false,
        suggestions: Vec::new(),
        line_comments: Vec::new(),
        verdict: Verdict::default(),
        sent: false,
        check_runs: Vec::new(),
        merged: None,
    }
}

fn jira(items: Vec<Item>) -> Remote {
    Remote {
        sources: vec![SourceData {
            source: Source::Jira,
            outcome: Outcome::Loaded(Loaded {
                items,
                ..Loaded::default()
            }),
        }],
    }
}

fn github(items: Vec<Item>, reviews: Vec<Review>) -> Remote {
    let prs = reviews
        .iter()
        .map(|r| PrLink {
            number: r.number,
            repo: "a/x".into(),
            head_sha: "abc".into(),
        })
        .collect();
    Remote {
        sources: vec![SourceData {
            source: Source::GitHub,
            outcome: Outcome::Loaded(Loaded {
                items,
                reviews,
                prs,
                warnings: Vec::new(),
            }),
        }],
    }
}

fn synced(remote: &Remote) -> Workspace {
    merge(&Workspace::default(), remote, T).0
}

#[test]
fn new_entries_come_in_front_and_the_demo_goes() {
    let (ws, changes) = merge(
        &demo::workspace(),
        &jira(vec![ticket("ABC-1", "Eins"), ticket("ABC-2", "Zwei")]),
        T,
    );
    let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["ABC-1", "ABC-2"], "Beispieldaten sind weg");
    assert!(ws.reviews.is_empty() && ws.runs.is_empty());
    assert!(ws.sync.demo_dropped);
    assert!(ws.sync.is_synced("abc-1"));
    assert_eq!(
        changes
            .iter()
            .filter(|c| matches!(c, Change::ItemAdded { .. }))
            .count(),
        2
    );
    // Newly added later stands in front, existing keeps its place.
    let (ws, _) = merge(
        &ws,
        &jira(vec![
            ticket("ABC-3", "Drei"),
            ticket("ABC-1", "Eins"),
            ticket("ABC-2", "Zwei"),
        ]),
        T + 1,
    );
    let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["ABC-3", "ABC-1", "ABC-2"]);
}

#[test]
fn removed_entries_vanish_but_other_sources_stay() {
    let mut ws = synced(&jira(vec![
        ticket("ABC-1", "Eins"),
        ticket("ABC-2", "Zwei"),
    ]));
    ws = merge(&ws, &github(vec![pr_item(5)], vec![review(5, vec![])]), T).0;
    let (ws, changes) = merge(&ws, &jira(vec![ticket("ABC-2", "Zwei")]), T + 1);
    assert!(ws.item("abc-1").is_none());
    assert!(ws.item("abc-2").is_some() && ws.item("gh-a-x-5").is_some());
    assert!(!ws.sync.is_synced("abc-1"));
    assert!(changes.contains(&Change::ItemRemoved {
        id: "abc-1".into(),
        key: "ABC-1".into()
    }));
    assert!(ws.review(5).is_some(), "GitHub is not affected");
}

#[test]
fn local_state_survives_a_sync() {
    let lines = [
        (1, LineKind::Context, "fn a() {"),
        (2, LineKind::Add, "    foo();"),
    ];
    let mut ws = synced(&github(vec![pr_item(5)], vec![review(5, files(&lines))]));
    ws = merge(&ws, &jira(vec![ticket("ABC-1", "Eins")]), T).0;
    // Local states …
    ws.mark_done("abc-1");
    ws.snooze("abc-1", Some(T + 10_000_000));
    ws.items[1].agent_note = Some("Agent wartet".into());
    ws.runs.push(AgentRun::new("ABC-1", "Eins"));
    let r = ws.reviews.iter_mut().find(|r| r.number == 5).unwrap();
    r.suggestions.push(Suggestion::new(
        "s1",
        "a.rs · Zeile 2",
        crate::Level::Note,
        "Hm",
    ));
    r.decide("s1", Some(Decision::Approved));
    r.set_verdict(Verdict::RequestChanges);
    r.add_line_comment("a.rs", 1, "Bitte prüfen", Some("    bar();"))
        .unwrap();

    // … stay on every sync.
    let remote = Remote {
        sources: [
            jira(vec![ticket("ABC-1", "Eins neu betitelt")]).sources,
            github(vec![pr_item(5)], vec![review(5, files(&lines))]).sources,
        ]
        .concat(),
    };
    let (after, _) = merge(&ws, &remote, T + 1);
    let item = after.item("abc-1").unwrap();
    assert_eq!(
        item.title, "Eins neu betitelt",
        "der Dienst bestimmt den Titel"
    );
    assert!(item.read, "erledigt bleibt erledigt");
    assert!(after.snooze_of("abc-1").is_some());
    assert_eq!(after.runs.len(), 1);
    assert_eq!(
        after.item("gh-a-x-5").unwrap().agent_note.as_deref(),
        Some("Agent wartet")
    );
    let r = after.review(5).unwrap();
    assert_eq!(r.suggestions[0].decision, Some(Decision::Approved));
    assert_eq!(r.verdict, Verdict::RequestChanges);
    assert_eq!(r.line_comments.len(), 1);
    assert_eq!(r.line_comments[0].suggestion.as_deref(), Some("    bar();"));
}

#[test]
fn sent_review_and_merge_state_stay() {
    let mut ws = synced(&github(vec![pr_item(5)], vec![review(5, vec![])]));
    let r = ws.reviews.iter_mut().find(|r| r.number == 5).unwrap();
    r.sent = true;
    r.merged = Some(crate::MergeMethod::Squash);
    let (after, _) = merge(
        &ws,
        &github(vec![pr_item(5)], vec![review(5, vec![])]),
        T + 1,
    );
    assert!(after.review(5).unwrap().sent);
    assert!(after.review(5).unwrap().merged.is_some());
}

#[test]
fn external_comment_is_logged_by_the_system_and_notifies() {
    let ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
    let mut ws = ws;
    ws.mark_done("abc-1");
    let mut remote_item = ticket("ABC-1", "Eins");
    remote_item.comments = vec![Comment::new(
        Person::new("Lena", "LK"),
        "gerade eben",
        "Kannst du das ansehen, @Du?",
    )];
    let before = ws.clone();
    let applied = ws
        .apply(
            Action::SyncRemote {
                remote: jira(vec![remote_item]),
            },
            Actor::System,
            T + 1,
        )
        .unwrap();
    assert_eq!(applied.logged, 1);
    let entry = ws.log.last().unwrap();
    assert_eq!(entry.actor, Actor::System);
    assert_eq!(entry.text, "Lena hat ABC-1 kommentiert");
    assert_eq!(entry.target, Some(Target::Item("abc-1".into())));
    assert!(!ws.item("abc-1").unwrap().read, "new to read");
    // notify works on before/after and sees the mention.
    let events = notify::events(&before, &ws);
    assert!(
        events
            .iter()
            .any(|e| e.reason == notify::NotifyReason::Mention),
        "{events:?}"
    );
}

#[test]
fn status_and_checks_changes_from_outside_are_reported() {
    let mut ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
    ws = merge(&ws, &github(vec![pr_item(5)], vec![review(5, vec![])]), T).0;
    ws.mark_done("abc-1");
    let mut t = ticket("ABC-1", "Eins");
    t.status = TicketStatus::Done;
    let mut pr = pr_item(5);
    pr.pr.as_mut().unwrap().checks = Checks::Failed;
    pr.pr.as_mut().unwrap().changes_requested = true;
    let remote = Remote {
        sources: [
            jira(vec![t]).sources,
            github(vec![pr], vec![review(5, vec![])]).sources,
        ]
        .concat(),
    };
    let (after, changes) = merge(&ws, &remote, T + 1);
    assert!(changes.iter().any(|c| matches!(
        c,
        Change::StatusChanged {
            to: TicketStatus::Done,
            ..
        }
    )));
    assert!(changes.iter().any(|c| matches!(
        c,
        Change::ChecksChanged {
            number: PrKey { number: 5, .. },
            to: Checks::Failed,
            ..
        }
    )));
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::ChangesRequested { number: PrKey { number: 5, .. }, .. }))
    );
    assert!(
        !after.item("abc-1").unwrap().read,
        "Änderung von außen holt zurück"
    );
    // Same state once more: no changes.
    let (_, again) = merge(&after, &remote, T + 2);
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn own_status_change_is_not_reported_back() {
    let mut ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
    ws.set_status("abc-1", TicketStatus::InProgress);
    ws.mark_done("abc-1");
    let mut t = ticket("ABC-1", "Eins");
    t.status = TicketStatus::InProgress;
    let (after, changes) = merge(&ws, &jira(vec![t]), T + 1);
    assert!(changes.is_empty());
    assert!(after.item("abc-1").unwrap().read);
}

#[test]
fn local_priority_and_pr_link_survive_the_sync() {
    let mut local = ticket("X-1", "Titel");
    local.priority = crate::model::Priority::Highest;
    local.pr = pr_item(7).pr;
    let mut changes = Vec::new();
    let merged = merge_item(local, &ticket("X-1", "Neuer Titel"), &mut changes);
    assert_eq!(merged.title, "Neuer Titel");
    assert_eq!(merged.priority, crate::model::Priority::Highest);
    assert_eq!(merged.pr.map(|p| p.number), Some(7));
}

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
    assert!(ws.item("abc-1").is_some(), "nichts geht verloren");
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
    assert_eq!(changes, vec![Change::Summary("8 neue Einträge".into())]);
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

// ---------- Equal numbers from two repos ----------

/// Pull request `number` of `repo` as item, review and link.
fn in_repo(repo: &str, number: u32, title: &str) -> (Item, Review, PrLink) {
    let mut item = pr_item(number);
    item.id = format!("gh-{}-{number}", repo.replace('/', "-"));
    item.repo = Some(repo.into());
    item.pr.as_mut().unwrap().repo = repo.into();
    let mut r = review(number, files(&[(1, LineKind::Add, "x")]));
    r.repo = repo.into();
    r.title = title.into();
    let link = PrLink {
        number,
        repo: repo.into(),
        head_sha: "abc".into(),
    };
    (item, r, link)
}

fn two_repos(title_b: &str) -> Remote {
    let (ia, ra, la) = in_repo("acme/app", 5, "In app");
    let (ib, rb, lb) = in_repo("acme/web", 5, title_b);
    Remote {
        sources: vec![SourceData {
            source: Source::GitHub,
            outcome: Outcome::Loaded(Loaded {
                items: vec![ia, ib],
                reviews: vec![ra, rb],
                prs: vec![la, lb],
                warnings: Vec::new(),
            }),
        }],
    }
}

#[test]
fn equal_numbers_from_two_repos_stay_two_reviews() {
    let ws = synced(&two_repos("In web"));
    assert_eq!(ws.reviews.len(), 2);
    assert_eq!(ws.review(PrKey::new("acme/app", 5)).unwrap().title, "In app");
    assert_eq!(ws.review(PrKey::new("acme/web", 5)).unwrap().title, "In web");
    assert_eq!(ws.sync.pr(PrKey::new("acme/web", 5)).unwrap().repo, "acme/web");
}

#[test]
fn local_state_stays_with_its_own_repo_across_syncs() {
    let mut ws = synced(&two_repos("In web"));
    let web = PrKey::new("acme/web", 5);
    ws.set_verdict(&web, Verdict::Approve);
    let (merged, _) = merge(&ws, &two_repos("In web, renamed"), T + 1);
    assert_eq!(merged.reviews.len(), 2);
    assert_eq!(merged.review(&web).unwrap().verdict, Verdict::Approve);
    assert_eq!(merged.review(&web).unwrap().title, "In web, renamed");
    assert_eq!(
        merged.review(PrKey::new("acme/app", 5)).unwrap().verdict,
        Verdict::default()
    );
}

#[test]
fn checks_changes_are_reported_for_the_right_repo() {
    let ws = synced(&two_repos("In web"));
    let mut next = two_repos("In web");
    if let Outcome::Loaded(l) = &mut next.sources[0].outcome {
        l.items[1].pr.as_mut().unwrap().checks = Checks::Failed;
    }
    let (_, changes) = merge(&ws, &next, T + 1);
    assert!(changes.iter().any(|c| matches!(
        c,
        Change::ChecksChanged { number, .. } if *number == PrKey::new("acme/web", 5)
    )));
    assert!(changes.iter().all(|c| match c {
        Change::ChecksChanged { number, .. } => number.repo == "acme/web",
        _ => true,
    }));
}

#[test]
fn an_old_state_with_one_repo_and_no_repo_fields_is_migrated() {
    // The format before several repos: reviews and references without `repo`, the link
    // `prs` carries the repo.
    let old = r##"{
        "items": [{"id":"gh-a-x-5","kind":"PullRequest","source":"GitHub","key":"#5","reason":"ReviewRequested",
                   "scope":"Mine","owner":{"name":"Lena","initials":"LK"},"repo":"acme/app",
                   "pr":{"number":5,"checks":"Passed"}}],
        "reviews": [{"number":5,"title":"Old","author":{"name":"Lena","initials":"LK"},"checks":"Passed"}],
        "sync": {"prs":[{"number":5,"repo":"acme/app","head_sha":"abc"}],"items":["gh-a-x-5"]}
    }"##;
    let mut ws: Workspace = serde_json::from_str(old).unwrap();
    assert_eq!(ws.reviews[0].repo, "");
    // It works even before the migration (a plain number finds the only review) …
    assert!(ws.review(5).is_some());
    // … and the migration fills the repo from the link and the entry.
    assert!(ws.fill_repos(None));
    assert_eq!(ws.reviews[0].repo, "acme/app");
    assert_eq!(ws.items[0].pr.as_ref().unwrap().repo, "acme/app");
    assert!(ws.review(PrKey::new("acme/app", 5)).is_some());
    assert!(ws.review(PrKey::new("acme/web", 5)).is_none());
}

#[test]
fn the_only_configured_repo_fills_reviews_without_any_other_hint() {
    let mut ws = Workspace {
        reviews: vec![review(7, Vec::new())],
        ..Workspace::default()
    };
    assert!(!ws.clone().fill_repos(None), "nothing to go by");
    assert!(ws.fill_repos(Some("acme/only")));
    assert_eq!(ws.reviews[0].repo, "acme/only");
}
