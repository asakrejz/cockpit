use super::*;

#[test]
fn linking_connects_both_sides() {
    let mut w = ws();
    // Do not follow the status, this is only about the link.
    w.pr_status_off = true;
    let applied = link(&mut w, "bau150", 85, 1);
    assert_eq!(
        applied.summary,
        "BAU-150 automatisch mit asakrejz/rust-playground#85 verknüpft"
    );
    let ticket = w.item("bau150").unwrap();
    assert_eq!(ticket.pr.as_ref().map(|p| p.number), Some(85));
    assert_eq!(ticket.branch.as_deref(), Some("feature/bau-150-x"));
    assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
    assert_eq!(w.ticket_of_pr(85).map(|t| t.id.as_str()), Some("bau150"));
    // The copy on the ticket corresponds to the pull request's entry (checks red).
    assert_eq!(ticket.pr.as_ref().unwrap().checks, Checks::Failed);
}

#[test]
fn the_manual_link_actions_are_gone_from_the_wire() {
    // Old clients may still send them; they are no known variant any more.
    for json in [
        r#"{"LinkPr":{"item":"bau150","pr":85}}"#,
        r#"{"UnlinkPr":{"item":"bau150"}}"#,
    ] {
        assert!(serde_json::from_str::<Action>(json).is_err());
    }
}

#[test]
fn old_states_with_hand_resolved_links_still_load() {
    let w = ws();
    let mut value: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
    value["workspace"]["unlinked"] =
        serde_json::json!([[{"repo": "acme/app", "number": 85}, "APP-1"]]);
    let (back, restored) = persist::restore(Some(&value.to_string()));
    assert!(restored);
    assert_eq!(back.items.len(), w.items.len());
}

#[test]
fn the_branch_or_title_links_automatically() {
    let mut w = ws();
    w.pr_status_off = true;
    name_pr85(&mut w, "feature/bau-150-sheet", "Sheet");
    assert_eq!(
        w.auto_links(),
        [("bau150".to_string(), w.review(85).unwrap().key())]
    );
    let applied = ok(&mut w, Action::AutoLinkPrs, 1);
    assert_eq!(
        applied.summary,
        "BAU-150 automatisch mit asakrejz/rust-playground#85 verknüpft"
    );
    let entry = w.log.last().unwrap();
    assert_eq!(
        entry.actor,
        Actor::System,
        "die Automatik handelt als System"
    );
    assert_eq!(
        w.item("bau150").unwrap().pr.as_ref().map(|p| p.number),
        Some(85)
    );
    assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
    // A second run finds nothing any more.
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);

    // It counts the title just the same.
    let mut w = ws();
    w.pr_status_off = true;
    name_pr85(&mut w, "mira/lint-fix", "BAU-150: Sheet");
    ok(&mut w, Action::AutoLinkPrs, 1);
    assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
}

#[test]
fn auto_linking_needs_the_whole_key() {
    let mut w = ws();
    name_pr85(&mut w, "feature/bau-1500-sheet", "Sheet");
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
    name_pr85(&mut w, "feature/xbau-150", "Sheet 150");
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
    // A pull request that already belongs to an existing ticket stays there.
    name_pr85(&mut w, "feature/bau-150-sheet", "Sheet");
    w.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .ticket_key = Some("BAU-142".into());
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
    // And a ticket with its own pull request takes no second one.
    w.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .ticket_key = None;
    w.items.iter_mut().find(|i| i.id == "bau150").unwrap().pr =
        w.item("bau138").unwrap().pr.clone();
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
}

#[test]
fn every_pr_naming_the_key_in_branch_title_or_body_belongs_to_the_ticket() {
    let mut w = ws();
    let key = w.item("bau150").unwrap().key.clone();
    let base = w.reviews.iter().find(|r| r.number == 85).unwrap().clone();
    let mut make = |number: u32, branch: &str, title: &str, body: &str| {
        let mut r = base.clone();
        r.number = number;
        r.branch = branch.into();
        r.title = title.into();
        r.description = body.into();
        r.ticket_key = None;
        r.merged = None;
        w.reviews.push(r);
    };
    make(901, "feature/bau-150-foo", "Foo", "");
    make(902, "mira/x", "BAU-150: Bar", "");
    make(903, "mira/y", "Baz", "Part of BAU-142 and bau-150.");
    make(904, "feature/bau-1500-x", "Other", "BAU-15000");
    let belongs: Vec<u32> = w
        .linked_reviews("bau150")
        .iter()
        .map(|r| r.number)
        .filter(|n| *n >= 900)
        .collect();
    assert_eq!(belongs, [901, 902, 903]);
    // One PR can belong to several tickets.
    assert!(
        w.reviews
            .iter()
            .find(|r| r.number == 903)
            .unwrap()
            .belongs_to("BAU-142")
    );
    assert!(
        w.reviews
            .iter()
            .find(|r| r.number == 903)
            .unwrap()
            .belongs_to(&key)
    );
    // Pull requests have no linked PRs of their own.
    assert!(w.linked_reviews("pr87").is_empty());
    let listed = w.linked_prs("bau150");
    assert!(
        listed
            .iter()
            .any(|l| l.pr.number == 901 && l.branch == "feature/bau-150-foo")
    );
}

#[test]
fn a_pr_the_agent_opened_stays_linked_without_the_key_in_its_text() {
    let mut w = ws();
    name_pr85(&mut w, "agent/sheet", "Sheet");
    w.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .ticket_key = Some("BAU-150".into());
    assert_eq!(
        w.linked_reviews("bau150")
            .iter()
            .map(|r| r.number)
            .collect::<Vec<_>>(),
        [85]
    );
}

#[test]
fn the_body_links_automatically_too() {
    let mut w = ws();
    w.pr_status_off = true;
    name_pr85(&mut w, "mira/lint-fix", "Sheet");
    w.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .description = "Closes BAU-150".into();
    ok(&mut w, Action::AutoLinkPrs, 1);
    assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
}
