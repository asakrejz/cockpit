use super::*;
use crate::agent::STEPS;
use crate::attention::Rule;
use crate::demo;
use crate::test_support::{advance, advance_at};

fn ws() -> Workspace {
    demo::workspace()
}

/// Options without plan approval: the agent runs immediately.
fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..Default::default()
    }
}

/// Lets all agents run to the end (the container reports every step) and answers every follow-up
/// question. Returns the keys of the runs that finished in the process.
fn finish_agents(w: &mut Workspace) -> Vec<String> {
    let done_before: Vec<String> = w
        .runs
        .iter()
        .filter(|r| r.is_done())
        .map(|r| r.key())
        .collect();
    for _ in 0..=STEPS {
        let asking: Vec<String> = w
            .runs
            .iter()
            .filter(|r| r.is_asking())
            .map(|r| r.ticket_key.clone())
            .collect();
        for key in asking {
            assert!(w.answer_agent(&key, "Entfernen"));
        }
        advance_at(w, 0);
    }
    w.runs
        .iter()
        .filter(|r| r.is_done() && !done_before.contains(&r.key()))
        .map(|r| r.key())
        .collect()
}

#[test]
fn demo_data_matches_the_design() {
    let w = ws();
    assert_eq!(w.items.len(), 7);
    assert_eq!(w.unread(Filter::All, NOW), 6, "pr85 is not mine");
    assert_eq!(
        (w.visible(Filter::Mine).len(), w.visible(Filter::Team).len()),
        (5, 2)
    );
    assert_eq!(w.review(87).unwrap().suggestions.len(), 3);
    assert_eq!(w.open_reviews(), 3);
    assert_eq!(w.running_agents(), 0);
    // Every pull request on an entry has a review.
    // (Exception: #12 in the second repo, which only shows the inbox filters.)
    for pr in w
        .items
        .iter()
        .filter_map(|i| i.pr.as_ref())
        .filter(|p| p.number != 12)
    {
        assert!(w.review(pr.number).is_some(), "#{}", pr.number);
    }
}

#[test]
fn done_items_stop_counting() {
    let mut w = ws();
    assert_eq!(w.unread(Filter::Mine, NOW), 5);
    w.mark_done("bau142");
    assert_eq!(w.unread(Filter::All, NOW), 5);
    assert_eq!(w.unread(Filter::Mine, NOW), 4);
    assert_eq!(w.unread(Filter::Team, NOW), 1);
}

#[test]
fn the_tasks_badge_counts_every_entry_once() {
    let mut w = ws();
    // "Ich" (5) is part of "Alle" (6): no double counting.
    assert_eq!(w.tasks_unread(NOW), 6);
    w.mark_done("bau142");
    assert_eq!(w.tasks_unread(NOW), 5);
    // Snoozed entries are not new.
    assert_eq!(
        w.tasks_unread(NOW),
        w.unread(Filter::Mine, NOW) + w.unread(Filter::Team, NOW)
    );
}

/// 11.11.2026, 12:00 UTC.
const NOW: u64 = 1_794_398_400_000;
const HOUR: u64 = 3_600_000;

fn ids(items: Vec<&Item>) -> Vec<&str> {
    items.into_iter().map(|i| i.id.as_str()).collect()
}

#[test]
fn the_inbox_follows_the_rules() {
    let mut w = ws();
    // pr85 (checks red, but from Mira) matches no rule and is missing.
    assert_eq!(
        ids(w.attention(Filter::All, NOW)),
        ["bau142", "pr87", "bau138", "bau150", "doc7", "pr12"]
    );
    assert_eq!(ids(w.attention(Filter::Team, NOW)), ["pr87"]);
    // Rule off: the hits behind it disappear – bau150 stays because of „Zugewiesen“.
    w.set_rule(Rule::ReviewRequested, false);
    assert_eq!(ids(w.attention(Filter::Team, NOW)), Vec::<&str>::new());
    w.set_rule(Rule::Overdue, false);
    w.set_rule(Rule::Assigned, false);
    assert!(!ids(w.attention(Filter::All, NOW)).contains(&"bau150"));
    // The reason of the entry is the first hit.
    assert_eq!(
        w.hits(w.item("bau142").unwrap(), NOW)[0].rule,
        Rule::Mention
    );
    // Answering resolves the mention.
    w.reply("bau142", "Mache ich.");
    w.set_rule(Rule::Assigned, true);
    assert_eq!(
        w.hits(w.item("bau142").unwrap(), NOW)[0].rule,
        Rule::Assigned
    );
}

#[test]
fn counts_follow_the_rules_and_snoozing() {
    let mut w = ws();
    assert_eq!(w.unread(Filter::Mine, NOW), 5);
    w.set_rule(Rule::Mention, false);
    w.set_rule(Rule::Assigned, false);
    // bau142 and doc7 now have no reason left; bau138 (blocked), bau150 (overdue)
    // and pr12 (blocked) stay.
    assert_eq!(w.unread(Filter::Mine, NOW), 3);
    assert!(w.snooze("bau138", Some(NOW + HOUR)));
    assert_eq!(w.unread(Filter::Mine, NOW), 2);
    assert_eq!(w.unread(Filter::Snoozed, NOW), 1);
    // After expiry it counts again – even if nobody calls `wake`.
    assert_eq!(w.unread(Filter::Mine, NOW + HOUR), 3);
    assert_eq!(w.unread(Filter::Snoozed, NOW + HOUR), 0);
}

#[test]
fn snoozed_items_leave_the_inbox_and_show_in_their_filter() {
    let mut w = ws();
    assert!(w.snooze("pr87", Some(NOW + HOUR)));
    assert!(!w.snooze("gibt-es-nicht", None));
    assert!(!ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
    assert_eq!(ids(w.attention(Filter::Snoozed, NOW)), ["pr87"]);
    assert_eq!(
        ids(w.search(Filter::Snoozed, SourceFilter::GitHub, "slider", NOW)),
        ["pr87"]
    );
    assert!(
        w.search(Filter::Snoozed, SourceFilter::Jira, "", NOW)
            .is_empty()
    );
    // Search and source work on the rules, not on all entries.
    assert!(
        w.search(Filter::All, SourceFilter::All, "pr85", NOW)
            .is_empty()
    );
    // Snoozing again replaces the old snooze.
    w.snooze("pr87", None);
    assert_eq!(w.snoozed.len(), 1);
    assert!(w.unsnooze("pr87"));
    assert!(!w.unsnooze("pr87"));
    assert!(ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
}

#[test]
fn wake_removes_only_what_expired() {
    let mut w = ws();
    w.snooze("pr87", Some(NOW + HOUR));
    w.snooze("bau150", Some(NOW + 5 * HOUR));
    w.snooze("bau138", None);
    assert!(w.wake(NOW).is_empty());
    assert_eq!(w.wake(NOW + 2 * HOUR), ["pr87"]);
    assert_eq!(w.snoozed.len(), 2);
    // „Until something changes“: a comment brings the entry back.
    w.reply("bau138", "Hallo");
    assert!(ids(w.attention(Filter::All, NOW)).contains(&"bau138"));
    assert_eq!(w.expired_snoozes(NOW), ["bau138"]);
    assert_eq!(w.wake(NOW), ["bau138"]);
    assert_eq!(w.snoozed.len(), 1);
}

#[test]
fn a_status_change_wakes_a_change_snooze() {
    let mut w = ws();
    w.snooze("bau150", None);
    assert!(w.is_snoozed("bau150", NOW + 1000 * HOUR));
    w.set_status("bau150", TicketStatus::InProgress);
    assert!(!w.is_snoozed("bau150", NOW));
}

#[test]
fn answering_a_review_request_ends_the_attention() {
    let mut w = ws();
    for (s, d) in [
        ("a", Decision::Approved),
        ("b", Decision::Approved),
        ("c", Decision::Approved),
    ] {
        w.decide(87, s, Some(d));
    }
    w.send_review(87, "").unwrap();
    assert!(!ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
}

#[test]
fn only_tickets_have_a_status() {
    let mut w = ws();
    assert!(w.set_status("bau150", TicketStatus::Done));
    assert_eq!(w.item("bau150").unwrap().status, TicketStatus::Done);
    assert!(!w.set_status("pr87", TicketStatus::Done));
    assert!(!w.set_status("gibt-es-nicht", TicketStatus::Done));
}

#[test]
fn branch_is_named_after_key_and_title() {
    let mut w = ws();
    assert_eq!(
        w.create_branch("bau150").as_deref(),
        Some("feature/BAU-150-sheet-detents-mittel-und")
    );
    assert_eq!(w.create_branch("bau150"), None, "there already is one");
    assert_eq!(w.create_branch("pr87"), None);
}

#[test]
fn replies_are_trimmed_and_mine() {
    let mut w = ws();
    assert!(!w.reply("bau142", "  \n "));
    assert!(w.reply("bau142", "  Mache ich.  "));
    let last = w.item("bau142").unwrap().comments.last().unwrap();
    assert_eq!(
        (last.text.as_str(), last.own, last.author.name.as_str()),
        ("Mache ich.", true, "Du")
    );
}

#[test]
fn sending_a_review_closes_the_inbox_item() {
    let mut w = ws();
    assert_eq!(w.send_review(87, ""), Err(SendError::Undecided(3)));
    assert_eq!(w.send_review(1, ""), Err(SendError::UnknownReview));
    w.decide(87, "a", Some(Decision::Approved));
    w.decide(87, "b", Some(Decision::Declined));
    w.decide(87, "c", Some(Decision::Approved));
    w.set_verdict(87, Verdict::RequestChanges);
    assert_eq!(w.send_review(87, " Bitte Fokus prüfen. "), Ok(2));

    let item = w.item("pr87").unwrap();
    let comment = item.comments.last().unwrap();
    assert_eq!(
        comment.text,
        "Review gesendet: Änderungen · 2 Kommentare\nBitte Fokus prüfen."
    );
    assert!(comment.own && item.read);
    assert_eq!(item.agent_note, None);
    assert_eq!(w.open_reviews(), 2);
}

#[test]
fn own_line_comments_go_out_with_the_review() {
    let mut w = ws();
    let file = w.review(87).unwrap().files[0].name.clone();
    assert_eq!(w.add_line_comment(1, &file, 0, "x", None), None);
    let id = w
        .add_line_comment(87, &file, 0, "Warum hier?", None)
        .unwrap();
    for s in ["a", "b", "c"] {
        w.decide(87, s, Some(Decision::Declined));
    }
    // Own comments alone count too, even if all suggestions are declined.
    assert_eq!(w.send_review(87, ""), Ok(1));
    assert_eq!(
        w.item("pr87").unwrap().comments.last().unwrap().text,
        "Review gesendet: Kommentieren · 1 Kommentar"
    );
    assert!(w.review(87).unwrap().sent);
    assert!(w.edit_line_comment(87, &id, "Anders", None));
    assert!(!w.review(87).unwrap().sent);
    assert!(w.delete_line_comment(87, &id));
    assert!(!w.delete_line_comment(1, &id));
}

#[test]
fn a_finished_agent_leaves_its_branch_and_waits_for_the_pull_request() {
    let mut w = ws();
    w.start_agent("BAU-142", direct()).unwrap();
    assert_eq!(w.running_agents(), 1);
    assert_eq!(w.item("bau142").unwrap().status, TicketStatus::InProgress);
    assert_eq!(
        w.start_agent("BAU-142", direct()),
        Err(AgentError::AlreadyRunning)
    );

    assert_eq!(finish_agents(&mut w), vec!["BAU-142".to_string()]);
    assert_eq!(w.running_agents(), 0);

    let item = w.item("bau142").unwrap();
    assert_eq!(
        item.branch.as_deref(),
        Some("claude/BAU-142-toggle-fokus-ring-fehlt")
    );
    assert_eq!(item.reason, ReasonKind::AgentFinished);
    assert!(!item.read, "a finished run needs attention");
    // The pull request is created only after your approval, so there is none yet.
    assert!(item.pr.is_none());
    let run = w.run("BAU-142").unwrap();
    assert!(run.is_done() && run.pr_number.is_none() && run.pull_request.is_none());
    assert!(w.check_pull_request("BAU-142", "").is_ok());
}

#[test]
fn two_agents_run_side_by_side() {
    let mut w = ws();
    w.start_agent("BAU-142", direct()).unwrap();
    w.start_agent("BAU-150", direct()).unwrap();
    assert_eq!(w.running_agents(), 2);
    let mut finished = finish_agents(&mut w);
    finished.sort();
    assert_eq!(finished, ["BAU-142", "BAU-150"]);
}

#[test]
fn a_cancelled_agent_leaves_no_traces() {
    let mut w = ws();
    w.start_agent("BAU-142", direct()).unwrap();
    advance(&mut w);
    w.cancel_agent("BAU-142");
    assert_eq!(w.running_agents(), 0);
    assert!(finish_agents(&mut w).is_empty());
    let item = w.item("bau142").unwrap();
    assert_eq!(
        (
            item.branch.clone(),
            item.pr.clone(),
            item.agent_note.clone()
        ),
        (None, None, None)
    );
    assert!(w.review(92).is_none());
    // A finished run can no longer be cancelled.
    w.cancel_agent("BAU-138");
    assert!(w.run("BAU-138").unwrap().is_done());
}

#[test]
fn a_plan_is_shown_before_anything_happens() {
    let mut w = ws();
    w.start_agent("BAU-142", AgentOptions::default()).unwrap();
    assert_eq!((w.running_agents(), w.waiting_agents()), (0, 1));
    assert_eq!(
        w.start_agent("BAU-142", AgentOptions::default()),
        Err(AgentError::AlreadyRunning)
    );
    // The tick makes no progress without approval.
    assert!(finish_agents(&mut w).is_empty());
    let run = w.run("BAU-142").unwrap();
    assert!(run.is_awaiting());
    assert_eq!(run.completed, 0);
    // Neither branch nor pull request nor new status.
    let item = w.item("bau142").unwrap();
    assert_eq!((item.branch.clone(), item.pr.clone()), (None, None));
    assert_eq!(item.status, ws().item("bau142").unwrap().status);
    assert_eq!(
        item.agent_note.as_deref(),
        Some("Der Agent wartet auf die Freigabe seines Plans")
    );
    assert!(w.review(92).is_none());
}

#[test]
fn approving_needs_the_plan_of_the_agent_and_then_continues() {
    let mut w = ws();
    w.start_agent("BAU-142", AgentOptions::default()).unwrap();
    // The agent has not presented its plan yet: nothing to approve.
    assert!(!w.approve_plan("BAU-142"));
    w.report_agent(
        "BAU-142",
        AgentReport::Plan {
            summary: "Fokus-Ring ergänzen".into(),
            files: vec!["src/toggle.rs".into()],
            steps: vec!["Ändern".into()],
        },
        0,
    )
    .unwrap();
    assert!(w.approve_plan("BAU-142"));
    assert!(!w.approve_plan("BAU-142"), "approve only once");
    assert_eq!((w.running_agents(), w.waiting_agents()), (1, 0));
    assert_eq!(finish_agents(&mut w), vec!["BAU-142".to_string()]);
    assert!(w.item("bau142").unwrap().branch.is_some());
}

#[test]
fn rejecting_a_plan_leaves_no_traces() {
    let mut w = ws();
    let before = w.clone();
    w.start_agent("BAU-142", AgentOptions::default()).unwrap();
    assert!(w.reject_plan("BAU-142"));
    assert!(!w.reject_plan("BAU-142"));
    assert_eq!((w.running_agents(), w.waiting_agents()), (0, 0));
    assert!(finish_agents(&mut w).is_empty());
    assert_eq!(w.item("bau142"), before.item("bau142"));
    assert!(w.review(92).is_none());
    assert!(w.run("BAU-142").unwrap().is_idle());
    // After that it can be restarted.
    assert!(w.start_agent("BAU-142", AgentOptions::default()).is_ok());
}

#[test]
fn a_waiting_plan_can_be_cancelled_and_revised() {
    let mut w = ws();
    w.start_agent("BAU-150", AgentOptions::default()).unwrap();
    assert!(!w.revise_plan("BAU-150", "  "), "empty does not count");
    assert!(w.revise_plan("BAU-150", "Auch die Doku anpassen"));
    assert!(w.run("BAU-150").unwrap().is_awaiting());
    w.cancel_agent("BAU-150");
    assert!(w.run("BAU-150").unwrap().is_idle());
    assert_eq!(w.item("bau150").unwrap().agent_note, None);
    assert!(!w.revise_plan("BAU-150", "zu spät"));
}

#[test]
fn agents_only_work_on_tickets() {
    let mut w = ws();
    assert_eq!(
        w.start_agent("#87", direct()),
        Err(AgentError::UnknownTicket)
    );
    assert_eq!(
        w.start_agent("NOPE-1", direct()),
        Err(AgentError::UnknownTicket)
    );
    assert!(w.run("#87").is_none());
    let fresh = w.run("BAU-150").unwrap();
    assert!(fresh.is_idle());
    assert!(
        fresh.files.is_empty(),
        "the run has not changed anything yet"
    );
}
