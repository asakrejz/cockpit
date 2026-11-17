use super::*;

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
