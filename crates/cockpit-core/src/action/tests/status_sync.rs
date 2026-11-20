use super::*;

// ---------- Status follows the pull request ----------

#[test]
fn the_status_follows_the_pull_request() {
    let mut w = ws();
    // Draft, no review requested → In progress.
    assert_eq!(status(&w, "bau150"), TicketStatus::Open);
    let applied = link(&mut w, "bau150", 85, 2);
    assert_eq!(
        applied.summary,
        "BAU-150 automatisch mit acme/app#85 verknüpft"
    );
    assert_eq!(status(&w, "bau150"), TicketStatus::InProgress);
    let entry = w.log.last().unwrap();
    assert_eq!(entry.actor, Actor::System);
    assert_eq!(
        entry.text,
        "Status von BAU-150 automatisch auf „In Arbeit“ gesetzt – acme/app#85 ist noch in Arbeit"
    );

    // Review requested → Review.
    let applied = ok(&mut w, Action::RequestReview { review: 85.into() }, 2);
    assert_eq!(applied.summary, "Review für #85 angefragt");
    assert_eq!(status(&w, "bau150"), TicketStatus::Review);
    let entry = w.log.last().unwrap();
    assert_eq!(
        (entry.actor, entry.text.as_str()),
        (
            Actor::System,
            "Status von BAU-150 automatisch auf „Review“ gesetzt – für acme/app#85 ist ein Review angefragt"
        )
    );
    fails(
        &mut w,
        Action::RequestReview { review: 85.into() },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::RequestReview { review: 999.into() },
        ActionError::UnknownReview(999.into()),
    );

    // Nothing new: the tick does nothing.
    fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
}

#[test]
fn merging_finishes_the_linked_ticket_through_the_same_path() {
    let mut w = ws();
    // #91 belongs to BAU-138.
    w.items
        .iter_mut()
        .find(|i| i.id == "bau138")
        .unwrap()
        .pr
        .as_mut()
        .unwrap()
        .draft = false;
    ok(
        &mut w,
        Action::SendReview {
            review: 91.into(),
            comment: String::new(),
        },
        1,
    );
    assert_eq!(status(&w, "bau138"), TicketStatus::Review);
    ok(&mut w, merge(91, MergeMethod::Squash), 2);
    let ticket = w.item("bau138").unwrap();
    assert_eq!(ticket.status, TicketStatus::Done);
    assert!(ticket.read && ticket.agent_note.is_none());
    let entry = w.log.last().unwrap();
    assert_eq!(entry.actor, Actor::System);
    assert!(entry.text.ends_with("– acme/app#91 ist gemergt"));
}

#[test]
fn a_status_set_by_hand_stays_until_the_pull_request_moves() {
    let mut w = ws();
    link(&mut w, "bau150", 85, 2);
    // Back to „Offen“ by hand: the automation does not overrule that.
    ok(
        &mut w,
        Action::SetStatus {
            item: "bau150".into(),
            status: TicketStatus::Open,
        },
        1,
    );
    fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
    assert_eq!(status(&w, "bau150"), TicketStatus::Open);
    // When the pull request moves, the ticket follows again.
    ok(&mut w, Action::RequestReview { review: 85.into() }, 2);
    assert_eq!(status(&w, "bau150"), TicketStatus::Review);
}

#[test]
fn a_jira_project_narrows_the_inbox() {
    use crate::{Filter, SourceFilter};
    let mut w = ws();
    let mut other = w.item_by_key("BAU-142").unwrap().clone();
    other.id = "web7".into();
    other.key = "WEB-7".into();
    w.items.push(other);
    assert_eq!(w.projects(), ["BAU", "DOC", "WEB"]);
    let all = w.search(Filter::All, SourceFilter::All, "", 0).len();

    let applied = ok(
        &mut w,
        Action::SelectJiraProject {
            project: Some(" web ".into()),
        },
        1,
    );
    assert_eq!(applied.summary, "Jira-Projekt gewählt: WEB");
    assert_eq!(w.jira_project.as_deref(), Some("WEB"));
    let hits = w.search(Filter::All, SourceFilter::All, "", 0);
    assert!(hits.iter().any(|i| i.key == "WEB-7"));
    assert!(hits.iter().all(|i| i.project() != Some("BAU")));
    // Pull requests belong to no project and stay visible.
    assert!(hits.iter().any(|i| !i.is_ticket()));
    fails(
        &mut w,
        Action::SelectJiraProject {
            project: Some("WEB".into()),
        },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::SelectJiraProject {
            project: Some("A) OR (B".into()),
        },
        ActionError::InvalidInput,
    );

    // The choice is a setting: it survives the demo reset.
    w.apply(Action::ResetDemo, Actor::User, T).unwrap();
    assert_eq!(w.jira_project.as_deref(), Some("WEB"));
    assert_eq!(w.projects(), ["BAU", "DOC", "WEB"]);

    let applied = ok(&mut w, Action::SelectJiraProject { project: None }, 1);
    assert_eq!(applied.summary, "Jira-Projekt: alle");
    assert!(w.search(Filter::All, SourceFilter::All, "", 0).len() < all);
}

#[test]
fn the_automation_can_be_switched_off() {
    let mut w = ws();
    let off = Action::SetStatusFollowsPr { enabled: false };
    let on = Action::SetStatusFollowsPr { enabled: true };
    let applied = ok(&mut w, off.clone(), 1);
    assert_eq!(
        applied.summary,
        "Status folgt dem Pull Request: ausgeschaltet"
    );
    fails(&mut w, off, ActionError::Unchanged);

    // Off: linking, requesting a review and merging leave the status alone.
    link(&mut w, "bau150", 85, 1);
    ok(&mut w, Action::RequestReview { review: 85.into() }, 1);
    assert_eq!(status(&w, "bau150"), TicketStatus::Open);
    fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
    w.items
        .iter_mut()
        .find(|i| i.id == "bau138")
        .unwrap()
        .pr
        .as_mut()
        .unwrap()
        .draft = false;
    ok(
        &mut w,
        Action::SendReview {
            review: 91.into(),
            comment: String::new(),
        },
        1,
    );
    ok(&mut w, merge(91, MergeMethod::Merge), 1);
    assert_eq!(
        status(&w, "bau138"),
        TicketStatus::Review,
        "the pull request is merged, the ticket stays"
    );
    assert!(w.item("bau138").unwrap().pr.as_ref().unwrap().merged);

    // The setting survives reset and saving.
    let (back, _) = persist::restore(Some(&persist::save(&w)));
    assert!(back.pr_status_off);
    w.apply(Action::ResetDemo, Actor::User, T).unwrap();
    assert!(w.pr_status_off);

    // On again: whatever has moved is brought in line, with the system as author.
    let mut w = back;
    let applied = ok(&mut w, on, 3);
    assert_eq!(
        applied.summary,
        "Status folgt dem Pull Request: eingeschaltet"
    );
    assert_eq!(status(&w, "bau150"), TicketStatus::Review);
    assert_eq!(status(&w, "bau138"), TicketStatus::Done);
    assert!(w.log.iter().rev().take(2).all(|e| e.actor == Actor::System));
}

#[test]
fn the_agent_publishes_through_the_same_status_rule() {
    let mut w = ws();
    w.apply(
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: direct(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    for _ in 0..STEPS {
        let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T);
    }
    let ticket = w.item("bau150").unwrap();
    assert_eq!(ticket.status, TicketStatus::Review);
    assert!(ticket.pr.as_ref().unwrap().review_requested);
    // The agent linked the ticket; the tick finds nothing left to do.
    assert!(w.ticket_of_pr(92).is_some());
    fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
    fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
}

#[test]
fn only_you_merge_even_with_the_automation() {
    let mut w = ws();
    for actor in [Actor::Agent, Actor::System] {
        let before = w.clone();
        assert_eq!(
            w.apply(merge(87, MergeMethod::Merge), actor, T),
            Err(ActionError::MergeNotAllowed(actor))
        );
        assert_eq!(w, before);
    }
    // The new actions never merge.
    link(&mut w, "bau150", 85, 2);
    let _ = w.apply(Action::SyncTicketStatus, Actor::System, T);
    assert!(w.reviews.iter().all(|r| r.merged.is_none()));
}

#[test]
fn the_new_ticket_actions_survive_json() {
    for a in [
        edit("bau150", "Titel", "Text"),
        assign("bau150", lena()),
        Action::SetPriority {
            item: "bau150".into(),
            priority: Priority::Lowest,
        },
        Action::AutoLinkPrs,
        Action::RequestReview { review: 85.into() },
        Action::SyncTicketStatus,
        Action::SetStatusFollowsPr { enabled: false },
    ] {
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    }
}
