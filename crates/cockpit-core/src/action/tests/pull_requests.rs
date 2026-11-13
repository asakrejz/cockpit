use super::*;

#[test]
fn linking_by_hand_connects_both_sides() {
    let mut w = ws();
    // Do not follow the status, this is only about the link.
    w.pr_status_off = true;
    let applied = ok(&mut w, link("bau150", 85), 1);
    assert_eq!(applied.summary, "BAU-150 mit #85 verknüpft");
    let ticket = w.item("bau150").unwrap();
    assert_eq!(ticket.pr.as_ref().map(|p| p.number), Some(85));
    assert_eq!(
        ticket.branch.as_deref(),
        Some("feature/BAU-127-list-separator")
    );
    assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
    assert_eq!(w.ticket_of_pr(85).map(|t| t.id.as_str()), Some("bau150"));
    // The copy on the ticket corresponds to the pull request's entry (checks red).
    assert_eq!(ticket.pr.as_ref().unwrap().checks, Checks::Failed);

    fails(&mut w, link("bau150", 85), ActionError::Unchanged);
    fails(
        &mut w,
        link("bau150", 87),
        ActionError::Link(LinkError::TicketHasPr(PrKey::new(
            "asakrejz/rust-playground",
            85,
        ))),
    );
    fails(
        &mut w,
        link("bau142", 85),
        ActionError::Link(LinkError::PrHasTicket("BAU-150".into())),
    );
    fails(
        &mut w,
        link("bau142", 999),
        ActionError::Link(LinkError::UnknownPr(999.into())),
    );
    fails(&mut w, link("pr87", 85), ActionError::NotATicket);
    fails(
        &mut w,
        link("nix", 85),
        ActionError::UnknownItem("nix".into()),
    );
}

#[test]
fn unlinking_removes_both_sides_and_the_branch_it_brought() {
    let mut w = ws();
    w.pr_status_off = true;
    ok(&mut w, link("bau150", 85), 1);
    let applied = ok(&mut w, unlink("bau150"), 1);
    assert_eq!(
        applied.summary,
        "Verknüpfung von BAU-150 mit asakrejz/rust-playground#85 gelöst"
    );
    let ticket = w.item("bau150").unwrap();
    assert_eq!((ticket.pr.clone(), ticket.branch.clone()), (None, None));
    assert_eq!(w.review(85).unwrap().ticket_key, None);
    assert!(w.ticket_of_pr(85).is_none());
    fails(&mut w, unlink("bau150"), ActionError::NothingToDo);
    fails(&mut w, unlink("pr87"), ActionError::NotATicket);
    // A branch of the ticket's own stays.
    w.items
        .iter_mut()
        .find(|i| i.id == "bau150")
        .unwrap()
        .branch = Some("mein-branch".into());
    ok(&mut w, link("bau150", 85), 1);
    ok(&mut w, unlink("bau150"), 1);
    assert_eq!(
        w.item("bau150").unwrap().branch.as_deref(),
        Some("mein-branch")
    );
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
fn a_link_removed_by_hand_is_not_set_again() {
    let mut w = ws();
    w.pr_status_off = true;
    name_pr85(&mut w, "feature/bau-150-sheet", "Sheet");
    ok(&mut w, Action::AutoLinkPrs, 1);
    ok(&mut w, unlink("bau150"), 1);
    assert_eq!(
        w.unlinked,
        [(w.review(85).unwrap().key(), "BAU-150".to_string())]
    );
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
    // Linking by hand still works and lifts the lock.
    ok(&mut w, link("bau150", 85), 1);
    assert!(w.unlinked.is_empty());
    // The lock survives saving.
    ok(&mut w, unlink("bau150"), 1);
    let (back, _) = persist::restore(Some(&persist::save(&w)));
    assert_eq!(back.unlinked, w.unlinked);
}
