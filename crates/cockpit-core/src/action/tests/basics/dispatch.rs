use super::*;
use cockpit_i18n::Lang;

#[test]
fn every_action_variant_is_applied_and_logged() {
    let mut w = ws();
    let file = w.review(87).unwrap().files[0].name.clone();
    let id = "bau142".to_string();
    let key = "BAU-142".to_string();

    ok(&mut w, Action::MarkDone { item: id.clone() }, 1);
    ok(
        &mut w,
        Action::SetStatus {
            item: "bau150".into(),
            status: TicketStatus::Done,
        },
        1,
    );
    ok(
        &mut w,
        Action::CreateBranch {
            item: "bau150".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::Reply {
            item: id.clone(),
            text: "Mache ich.".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::Decide {
            review: 87.into(),
            suggestion: "a".into(),
            decision: Some(Decision::Approved),
        },
        1,
    );
    ok(
        &mut w,
        Action::EditSuggestion {
            review: 87.into(),
            suggestion: "a".into(),
            text: "Anders".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::AddLineComment {
            review: 87.into(),
            file,
            line: 0,
            text: "Warum?".into(),
            suggestion: None,
        },
        1,
    );
    let comment = w.review(87).unwrap().line_comments[0].id.clone();
    ok(
        &mut w,
        Action::EditLineComment {
            review: 87.into(),
            comment: comment.clone(),
            text: "Wieso?".into(),
            suggestion: None,
        },
        1,
    );
    ok(
        &mut w,
        Action::DeleteLineComment {
            review: 87.into(),
            comment,
        },
        1,
    );
    ok(
        &mut w,
        Action::SetVerdict {
            review: 87.into(),
            verdict: Verdict::Approve,
        },
        1,
    );
    for s in ["b", "c"] {
        w.decide(87, s, Some(Decision::Declined));
    }
    ok(
        &mut w,
        Action::SendReview {
            review: 87.into(),
            comment: String::new(),
        },
        1,
    );
    ok(
        &mut w,
        Action::StartAgent {
            ticket: key.clone(),
            options: AgentOptions::default(),
        },
        2,
    );
    plan_ready(&mut w, &key);
    ok(
        &mut w,
        Action::RevisePlan {
            ticket: key.clone(),
            note: "Doku".into(),
        },
        1,
    );
    plan_ready(&mut w, &key);
    ok(
        &mut w,
        Action::ApprovePlan {
            ticket: key.clone(),
        },
        1,
    );
    advance(&mut w, 1);
    ok(
        &mut w,
        Action::SteerAgent {
            run: key.clone(),
            text: "Auch den Slider".into(),
        },
        1,
    );
    ok(&mut w, Action::PauseAgent { run: key.clone() }, 1);
    ok(&mut w, Action::ResumeAgent { run: key.clone() }, 1);
    report(
        &mut w,
        &key,
        AgentReport::Question {
            text: "Remove it or mark it as deprecated?".into(),
            suggestions: Vec::new(),
        },
    )
    .unwrap();
    ok(
        &mut w,
        Action::AnswerAgent {
            run: key.clone(),
            text: "Entfernen".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::CancelAgent {
            ticket: key.clone(),
        },
        1,
    );
    ok(
        &mut w,
        Action::StartAgent {
            ticket: key.clone(),
            options: AgentOptions::default(),
        },
        2,
    );
    plan_ready(&mut w, &key);
    ok(&mut w, Action::RejectPlan { ticket: key }, 1);
}

#[test]
fn entries_carry_time_actor_target_and_a_german_sentence() {
    let mut w = ws();
    w.apply(
        Action::MarkDone {
            item: "bau142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    let e = &w.log[0];
    assert_eq!(
        (
            e.id,
            e.at_ms,
            e.actor,
            e.target.clone(),
            e.text_in(Lang::De).as_str()
        ),
        (
            1,
            T,
            Actor::User,
            Some(Target::Item("bau142".into())),
            "BAU-142 als erledigt markiert"
        )
    );
    w.apply(
        Action::SetStatus {
            item: "bau150".into(),
            status: TicketStatus::InProgress,
        },
        Actor::User,
        T + 5,
    )
    .unwrap();
    assert_eq!(w.log[1].id, 2);
    assert_eq!(
        w.log[1].text_in(Lang::De),
        "Status von BAU-150 auf „In Arbeit“ ggesetzt".replace("gg", "g")
    );
    assert_eq!(w.log[1].at_ms, T + 5);
}

#[test]
fn failures_are_returned_and_leave_no_entry() {
    let mut w = ws();
    fails(
        &mut w,
        Action::MarkDone {
            item: "gibt-es-nicht".into(),
        },
        ActionError::UnknownItem("gibt-es-nicht".into()),
    );
    w.mark_done("bau142");
    fails(
        &mut w,
        Action::MarkDone {
            item: "bau142".into(),
        },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::SetStatus {
            item: "pr87".into(),
            status: TicketStatus::Done,
        },
        ActionError::NotATicket,
    );
    fails(
        &mut w,
        Action::SetStatus {
            item: "bau150".into(),
            status: ws().item("bau150").unwrap().status,
        },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::CreateBranch {
            item: "pr87".into(),
        },
        ActionError::BranchExists,
    );
    fails(
        &mut w,
        Action::Reply {
            item: "bau142".into(),
            text: "  ".into(),
        },
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        Action::Decide {
            review: 1.into(),
            suggestion: "a".into(),
            decision: None,
        },
        ActionError::UnknownReview(1.into()),
    );
    fails(
        &mut w,
        Action::Decide {
            review: 87.into(),
            suggestion: "zzz".into(),
            decision: None,
        },
        ActionError::UnknownSuggestion("zzz".into()),
    );
    fails(
        &mut w,
        Action::AddLineComment {
            review: 87.into(),
            file: "nope.css".into(),
            line: 0,
            text: "x".into(),
            suggestion: None,
        },
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        Action::DeleteLineComment {
            review: 87.into(),
            comment: "c9".into(),
        },
        ActionError::UnknownComment("c9".into()),
    );
    fails(
        &mut w,
        Action::SendReview {
            review: 87.into(),
            comment: String::new(),
        },
        ActionError::Send(SendError::Undecided(3)),
    );
    fails(
        &mut w,
        Action::SendReview {
            review: 1.into(),
            comment: String::new(),
        },
        ActionError::UnknownReview(1.into()),
    );
    fails(
        &mut w,
        Action::StartAgent {
            ticket: "#87".into(),
            options: direct(),
        },
        ActionError::Agent(AgentError::UnknownTicket),
    );
    fails(
        &mut w,
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::CancelAgent {
            ticket: "BAU-138".into(),
        },
        ActionError::NothingToDo,
    );
    assert!(w.log.is_empty());
    assert!(!ActionError::NothingToDo.msg().render(Lang::De).is_empty());
}

#[test]
fn starting_twice_fails() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    fails(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        ActionError::Agent(AgentError::AlreadyRunning),
    );
}

#[test]
fn what_the_container_reports_is_logged_as_agent() {
    let mut w = ws();
    w.apply(
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Started {
            detail: cockpit_i18n::Msg::raw("docker"),
        },
    )
    .unwrap();
    advance(&mut w, 2);
    report(
        &mut w,
        "BAU-142",
        AgentReport::Question {
            text: "Remove it or mark it as deprecated?".into(),
            suggestions: Vec::new(),
        },
    )
    .unwrap();
    w.apply(
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "Remove it".into(),
        },
        Actor::User,
        T + 1,
    )
    .unwrap();
    advance(&mut w, STEPS as u64);
    assert!(w.run("BAU-142").unwrap().is_done());
    // The start and the answer are the user's, everything else the agent's.
    assert_eq!(
        w.log_entries(Some(Actor::User)).len(),
        2,
        "start and answer"
    );
    assert_eq!(w.log_entries(Some(Actor::System)).len(), 0);
    let all = w.log_entries(None).len();
    assert_eq!(w.log_entries(Some(Actor::Agent)).len(), all - 2);
    assert!(
        w.log
            .iter()
            .all(|e| e.target == Some(Target::Run("BAU-142".into())))
    );
    assert_eq!(w.log_entries(None)[0].id as usize, all, "newest first");
    // After that the run takes no more reports.
    assert!(
        report(
            &mut w,
            "BAU-142",
            AgentReport::Done {
                summary: cockpit_i18n::Msg::raw("again")
            }
        )
        .is_err()
    );
}

#[test]
fn the_two_rules_still_hold() {
    let mut w = ws();
    // Approval before sending.
    assert_eq!(
        w.apply(
            Action::SendReview {
                review: 87.into(),
                comment: String::new()
            },
            Actor::User,
            T
        ),
        Err(ActionError::Send(SendError::Undecided(
            w.review(87).unwrap().undecided()
        )))
    );
    // The agent never merges: no merge in the actions, the pull request waits as a review.
    w.apply(
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: direct(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    advance(&mut w, STEPS as u64);
    // A finished run opens no pull request on its own, so nothing was sent or merged.
    assert!(w.review(92).is_none());
    assert!(w.reviews.iter().all(|r| r.merged.is_none()));
}
