use super::*;
use crate::{AgentOptions, demo};

/// Without a plan step: the agent gets going right away.
fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    }
}

#[test]
fn a_plan_waiting_for_approval_is_stuck_with_me() {
    let mut w = demo::workspace();
    w.set_status("bau150", TicketStatus::InProgress);
    w.start_agent("BAU-150", AgentOptions::default()).unwrap();
    let s = stuck(&w);
    let entry = s.iter().find(|s| s.key == "BAU-150").unwrap();
    assert_eq!(entry.kind, StuckKind::PlanWaiting);
    assert_eq!(entry.who, Person::me());
    assert_eq!(entry.plan.as_deref(), Some("BAU-150"));
    // The waiting agent occupies the ticket.
    assert_eq!(member(&members(&w), "Agent").in_progress, 1);
    // After approval it runs – then nothing is stuck any more.
    assert!(w.approve_plan("BAU-150"));
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
}

fn member<'a>(list: &'a [Member], name: &str) -> &'a Member {
    list.iter().find(|m| m.person.name == name).unwrap()
}

#[test]
fn demo_members_are_grouped_by_person() {
    let list = members(&demo::workspace());
    let names: Vec<_> = list.iter().map(|m| m.person.name.as_str()).collect();
    assert_eq!(names, ["Du", "Agent", "Jonas", "Mira"]);

    let me = member(&list, "Du");
    assert_eq!(
        (me.open, me.in_progress, me.review, me.pull_requests),
        (2, 1, 1, 1)
    );
    assert_eq!(me.item_ids, ["bau142", "bau138", "bau150", "doc7", "pr12"]);
    // The agent has finished BAU-138, the review is waiting.
    let agent = member(&list, "Agent");
    assert_eq!((agent.in_progress, agent.review), (0, 1));
    assert_eq!(agent.item_ids, ["bau138"]);
    assert_eq!(member(&list, "Jonas").pull_requests, 1);
    assert_eq!(member(&list, "Mira").item_ids, ["pr85"]);
}

#[test]
fn me_and_agent_are_always_listed() {
    let list = members(&Workspace::default());
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|m| m.total() == 0));
    assert_eq!(list[0].load_level(), LoadLevel::Free);
    assert_eq!(list[0].load_percent(), 0);
}

#[test]
fn load_grows_with_open_entries() {
    let mut m = Member::new(Person::me());
    let mut levels = Vec::new();
    for _ in 0..=CAPACITY + 1 {
        levels.push((m.load_level(), m.load_percent()));
        m.open += 1;
    }
    assert_eq!(levels[0], (LoadLevel::Free, 0));
    assert_eq!(levels[1], (LoadLevel::Free, 25));
    assert_eq!(levels[2], (LoadLevel::Busy, 50));
    assert_eq!(levels[3], (LoadLevel::Busy, 75));
    assert_eq!(levels[CAPACITY], (LoadLevel::Full, 100));
    // Beyond the capacity the bar stays at 100.
    assert_eq!(levels[CAPACITY + 1], (LoadLevel::Full, 100));
}

#[test]
fn done_entries_do_not_count() {
    let mut w = demo::workspace();
    w.mark_done("bau150");
    w.set_status("bau142", TicketStatus::Done);
    let list = members(&w);
    assert_eq!(member(&list, "Du").item_ids, ["bau138", "doc7", "pr12"]);
}

#[test]
fn running_agent_counts_as_in_progress() {
    let mut w = demo::workspace();
    w.start_agent("BAU-150", direct()).unwrap();
    let list = members(&w);
    let agent = member(&list, "Agent");
    assert_eq!((agent.in_progress, agent.review), (1, 1));
    assert_eq!(agent.item_ids, ["bau138", "bau150"]);
    // When cancelled it no longer counts.
    w.cancel_agent("BAU-150");
    assert_eq!(member(&members(&w), "Agent").item_ids, ["bau138"]);
}

#[test]
fn demo_stuck_entries_in_order_of_urgency() {
    let list = stuck(&demo::workspace());
    let found: Vec<_> = list.iter().map(|s| (s.key.as_str(), s.kind)).collect();
    assert_eq!(
        found,
        [
            ("#85", StuckKind::ChecksFailed),
            ("#12", StuckKind::ChecksFailed),
            ("BAU-138", StuckKind::AgentWaiting),
            ("#87", StuckKind::ReviewWaiting),
            ("BAU-142", StuckKind::NoBranch),
        ]
    );
    // BAU-150 is only open and not stuck.
    assert!(list.iter().all(|s| s.key != "BAU-150"));
}

#[test]
fn stuck_entries_name_reason_and_target() {
    let list = stuck(&demo::workspace());
    let by_key = |k: &str| list.iter().find(|s| s.key == k).unwrap();

    let failed = by_key("#85");
    assert_eq!(failed.who.name, "Mira");
    assert!(failed.reason.de().contains("Mira"));
    assert_eq!(failed.review.as_ref().map(|k| k.number), Some(85));

    let agent = by_key("BAU-138");
    assert_eq!(agent.who, Person::me());
    assert!(agent.reason.de().contains("#91"));
    assert_eq!(agent.review.as_ref().map(|k| k.number), Some(91));

    let review = by_key("#87");
    assert!(review.reason.de().starts_with("Jonas wartet"));

    let branch = by_key("BAU-142");
    assert_eq!(branch.review, None);
    assert!(branch.reason.de().contains("keinen Branch"));
}

#[test]
fn creating_a_branch_unblocks_the_ticket() {
    let mut w = demo::workspace();
    w.create_branch("bau142");
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-142"));
}

#[test]
fn sending_the_review_unblocks_the_pull_request() {
    let mut w = demo::workspace();
    for s in ["a", "b", "c"] {
        w.decide(87, s, Some(crate::Decision::Approved));
    }
    w.send_review(87, "").unwrap();
    assert!(stuck(&w).iter().all(|s| s.key != "#87"));
    // For Jonas nothing is open any more; whoever has nothing does not appear any more.
    assert!(
        members(&w)
            .iter()
            .all(|m| m.person.name != "Jonas" || m.total() == 0)
    );
}

#[test]
fn finished_agent_run_waits_for_review_until_sent() {
    let mut w = demo::workspace();
    w.start_agent("BAU-150", direct()).unwrap();
    // While it is still running, nothing is stuck – even if the branch only comes into being at the end.
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
    for _ in 0..crate::agent::STEPS {
        w.advance_agents();
    }
    let s = stuck(&w);
    let entry = s.iter().find(|s| s.key == "BAU-150").unwrap();
    assert_eq!(entry.kind, StuckKind::AgentWaiting);
    assert_eq!(entry.who, Person::me());
}

#[test]
fn failed_checks_win_over_other_reasons() {
    let mut w = demo::workspace();
    // PR 87 is waiting for review – if the checks fail, that is the reason.
    w.items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .and_then(|i| i.pr.as_mut())
        .unwrap()
        .checks = Checks::Failed;
    let list = stuck(&w);
    let entry = list.iter().find(|s| s.key == "#87").unwrap();
    assert_eq!(entry.kind, StuckKind::ChecksFailed);
    assert_eq!(list.iter().filter(|s| s.key == "#87").count(), 1);
}

#[test]
fn drafts_and_running_checks_do_not_hang() {
    let mut w = demo::workspace();
    let pr = w
        .items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .and_then(|i| i.pr.as_mut())
        .unwrap();
    pr.draft = true;
    assert!(stuck(&w).iter().all(|s| s.key != "#87"));
    let pr = w
        .items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .and_then(|i| i.pr.as_mut())
        .unwrap();
    pr.draft = false;
    pr.checks = Checks::Running;
    assert!(stuck(&w).iter().all(|s| s.key != "#87"));
}

#[test]
fn state_label_for_tickets_and_pull_requests() {
    let w = demo::workspace();
    let label = |id: &str| state_label(w.item(id).unwrap());
    assert_eq!(label("bau150"), ("Offen".to_string(), Tone::Gray));
    assert_eq!(label("bau142"), ("In Arbeit".to_string(), Tone::Blue));
    assert_eq!(label("bau138"), ("Review".to_string(), Tone::Orange));
    assert_eq!(label("pr85").1, Tone::Red);
    assert_eq!(label("pr87").1, Tone::Orange);
}

#[test]
fn nothing_hangs_in_an_empty_workspace() {
    assert!(stuck(&Workspace::default()).is_empty());
}

#[test]
fn own_entries_read_naturally() {
    let mut w = demo::workspace();
    w.items.iter_mut().find(|i| i.id == "pr85").unwrap().owner = Person::me();
    let list = stuck(&w);
    let entry = list.iter().find(|s| s.key == "#85").unwrap();
    assert!(entry.reason.de().contains("Du kommst nicht weiter"));
}

#[test]
fn a_waiting_question_is_stuck_with_me() {
    let mut w = demo::workspace();
    w.start_agent("BAU-142", direct()).unwrap();
    for _ in 0..3 {
        w.advance_agents();
    }
    let s = stuck(&w);
    let entry = s.iter().find(|s| s.key == "BAU-142").unwrap();
    assert_eq!(entry.kind, StuckKind::QuestionWaiting);
    assert_eq!(entry.kind, StuckKind::QuestionWaiting);
    assert_eq!(entry.who, Person::me());
    assert_eq!(entry.question.as_deref(), Some("BAU-142"));
    assert!(entry.plan.is_none());
    assert!(
        entry.reason.de().contains("deprecated"),
        "{}",
        entry.reason.de()
    );
    assert_eq!(w.waiting_agents(), 1, "counts in the sidebar under agents");
    // A question is more urgent than a waiting plan.
    assert!(StuckKind::QuestionWaiting < StuckKind::PlanWaiting);
    // After the answer nothing is stuck any more.
    assert!(w.answer_agent("BAU-142", "Entfernen"));
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-142"));
    assert_eq!(w.waiting_agents(), 0);
}
