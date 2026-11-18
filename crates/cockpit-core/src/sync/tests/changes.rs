use super::*;

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
    assert!(changes.iter().any(|c| matches!(
        c,
        Change::ChangesRequested {
            number: PrKey { number: 5, .. },
            ..
        }
    )));
    assert!(
        !after.item("abc-1").unwrap().read,
        "a change from outside brings it back"
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
