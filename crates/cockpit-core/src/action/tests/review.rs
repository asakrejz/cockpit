use super::*;

#[test]
fn merging_is_refused_until_every_condition_holds() {
    let mut w = ws();
    fails(
        &mut w,
        merge(87, MergeMethod::Squash),
        ActionError::MergeBlocked(vec![MergeBlocker::ReviewNotSent]),
    );
    // A verdict „Changes“ blocks even after sending.
    ok(
        &mut w,
        Action::SetVerdict {
            review: 87,
            verdict: Verdict::RequestChanges,
        },
        1,
    );
    ready_87(&mut w);
    fails(
        &mut w,
        merge(87, MergeMethod::Merge),
        ActionError::MergeBlocked(vec![MergeBlocker::ChangesRequested]),
    );
    // Red checks (#85) block, the justification is in the error message.
    let err = w
        .apply(merge(85, MergeMethod::Merge), Actor::User, T)
        .unwrap_err();
    assert!(err.message().contains("Check ist fehlgeschlagen"));
    // Unknown.
    fails(
        &mut w,
        merge(1, MergeMethod::Merge),
        ActionError::UnknownReview(1),
    );
}

#[test]
fn a_paused_agent_makes_no_progress_until_resumed() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: direct(),
        },
        1,
    );
    ok(
        &mut w,
        Action::PauseAgent {
            run: "BAU-150".into(),
        },
        1,
    );
    assert_eq!(
        w.item("bau150").unwrap().agent_note.as_deref(),
        Some("Der Agent ist pausiert")
    );
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    fails(
        &mut w,
        Action::PauseAgent {
            run: "BAU-150".into(),
        },
        ActionError::NothingToDo,
    );
    // Cancelling works while paused too.
    ok(
        &mut w,
        Action::ResumeAgent {
            run: "BAU-150".into(),
        },
        1,
    );
    ok(&mut w, Action::AdvanceAgents, 1);
    ok(
        &mut w,
        Action::PauseAgent {
            run: "BAU-150".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::CancelAgent {
            ticket: "BAU-150".into(),
        },
        1,
    );
    assert!(w.run("BAU-150").unwrap().is_idle());
}

#[test]
fn the_agent_still_never_merges_after_questions() {
    let mut w = asking_ws();
    ok(
        &mut w,
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "Entfernen".into(),
        },
        1,
    );
    for _ in 0..STEPS {
        let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T);
    }
    // In the end a pull request waits for the review, nothing is merged.
    let review = w.review(92).unwrap();
    assert!(!review.sent);
    assert!(
        w.log
            .iter()
            .all(|e| !e.text.to_lowercase().contains("gemergt"))
    );
}

#[test]
fn new_actions_survive_json() {
    for action in [
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "Ja".into(),
        },
        Action::SteerAgent {
            run: "BAU-142".into(),
            text: "Ja".into(),
        },
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
    ] {
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), action);
    }
}

#[test]
fn only_you_may_merge() {
    let mut w = ws();
    ready_87(&mut w);
    let before = w.clone();
    for actor in [Actor::Agent, Actor::System] {
        let err = w
            .apply(merge(87, MergeMethod::Merge), actor, T)
            .unwrap_err();
        assert_eq!(err, ActionError::MergeNotAllowed(actor));
        assert!(err.message().starts_with("Nur du darfst mergen"));
    }
    assert_eq!(w, before, "no merge, no log entry");
    w.apply(merge(87, MergeMethod::Merge), Actor::User, T)
        .unwrap();
    assert!(w.review(87).unwrap().merged.is_some());
}

#[test]
fn merging_marks_the_pull_request_and_finishes_the_ticket() {
    let mut w = ws();
    // #91 belongs to BAU-138; the draft is approved beforehand.
    w.items
        .iter_mut()
        .find(|i| i.id == "bau138")
        .unwrap()
        .pr
        .as_mut()
        .unwrap()
        .draft = true;
    fails(
        &mut w,
        merge(91, MergeMethod::Rebase),
        ActionError::MergeBlocked(vec![MergeBlocker::Draft, MergeBlocker::ReviewNotSent]),
    );
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
            review: 91,
            comment: String::new(),
        },
        1,
    );
    let applied = ok(&mut w, merge(91, MergeMethod::Rebase), 2);
    assert_eq!(applied.summary, "#91 gemergt (Rebase)");
    // The automation updates the ticket, with the system as author.
    let entry = w.log.last().unwrap();
    assert_eq!(entry.actor, Actor::System);
    assert_eq!(entry.target, Some(Target::Item("bau138".into())));
    assert_eq!(
        entry.text,
        "Status von BAU-138 automatisch auf „Fertig“ gesetzt – #91 ist gemergt"
    );
    assert_eq!(w.review(91).unwrap().merged, Some(MergeMethod::Rebase));
    let ticket = w.item("bau138").unwrap();
    assert_eq!(ticket.status, TicketStatus::Done);
    assert!(ticket.read);
    let pr = ticket.pr.as_ref().unwrap();
    assert!(pr.merged);
    assert_eq!(crate::team::state_label(ticket).0, "Fertig");
    assert_eq!(pr.status().0, "Gemergt");
    // Merging twice is not possible, changing neither.
    fails(
        &mut w,
        merge(91, MergeMethod::Merge),
        ActionError::MergeBlocked(vec![MergeBlocker::AlreadyMerged]),
    );
    fails(
        &mut w,
        Action::SetVerdict {
            review: 91,
            verdict: Verdict::Approve,
        },
        ActionError::Merged(91),
    );
}

#[test]
fn merging_a_pull_request_without_ticket_marks_its_entry() {
    let mut w = ws();
    ready_87(&mut w);
    ok(&mut w, merge(87, MergeMethod::Squash), 1);
    let item = w.item("pr87").unwrap();
    assert_eq!(item.pr.as_ref().unwrap().label(), "#87 · gemergt");
    assert_eq!(
        crate::team::state_label(item),
        ("Gemergt".into(), Tone::Indigo)
    );
    assert!(item.read);
    assert_eq!(item.reason_text, "Gemergt (Squash)");
    let entry = w.log.last().unwrap();
    assert_eq!(
        (entry.actor, &entry.target),
        (Actor::User, &Some(Target::Review(87)))
    );
}

#[test]
fn a_failed_check_can_be_restarted_and_turns_green_after_a_few_ticks() {
    let mut w = ws();
    assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
    fails(
        &mut w,
        Action::RestartCheck {
            review: 85,
            check: "Build".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::RestartCheck {
            review: 85,
            check: "Gibt es nicht".into(),
        },
        ActionError::UnknownCheck("Gibt es nicht".into()),
    );
    fails(&mut w, Action::AdvanceChecks, ActionError::NothingToDo);
    ok(
        &mut w,
        Action::RestartCheck {
            review: 85,
            check: "Tests (Browser)".into(),
        },
        1,
    );
    assert_eq!(w.running_checks(), 1);
    for _ in 0..CHECK_STEPS - 1 {
        ok(&mut w, Action::AdvanceChecks, 0);
    }
    let applied = ok(&mut w, Action::AdvanceChecks, 1);
    assert_eq!(applied.summary, "Check „Tests (Browser)“ in #85 bestanden");
    assert_eq!(w.log.last().unwrap().actor, Actor::System);
    assert_eq!(w.running_checks(), 0);
    // The lint error is real: the overall state stays red.
    assert_eq!(w.review(85).unwrap().checks, Checks::Failed);

    // After the restart lint stays red …
    ok(
        &mut w,
        Action::RestartCheck {
            review: 85,
            check: "Lint".into(),
        },
        1,
    );
    for _ in 0..CHECK_STEPS {
        let _ = w.apply(Action::AdvanceChecks, Actor::System, T);
    }
    assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
    assert!(w.log.last().unwrap().text.contains("wieder fehlgeschlagen"));
}

#[test]
fn green_checks_flow_into_the_inbox_entry() {
    let mut w = ws();
    // Lint „repaired“: the restart ends green.
    w.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .check_runs[1]
        .rerun = Checks::Passed;
    for check in ["Lint", "Tests (Browser)"] {
        ok(
            &mut w,
            Action::RestartCheck {
                review: 85,
                check: check.into(),
            },
            1,
        );
    }
    assert_eq!(
        w.item("pr85").unwrap().pr.as_ref().unwrap().checks,
        Checks::Running
    );
    for _ in 0..CHECK_STEPS {
        let _ = w.apply(Action::AdvanceChecks, Actor::System, T);
    }
    assert_eq!(w.review(85).unwrap().checks, Checks::Passed);
    let item = w.item("pr85").unwrap();
    assert_eq!(item.pr.as_ref().unwrap().checks, Checks::Passed);
    assert_eq!(item.reason, ReasonKind::ReviewRequested);
    // Now only the sent review is missing.
    assert_eq!(w.merge_blockers(85), vec![MergeBlocker::ReviewNotSent]);
}

#[test]
fn line_comment_suggestions_go_through_the_actions() {
    let mut w = ws();
    let file = w.review(87).unwrap().files[0].name.clone();
    // Position 8: `  background: #ffffff;` (new line).
    ok(
        &mut w,
        Action::AddLineComment {
            review: 87,
            file: file.clone(),
            line: 5,
            text: String::new(),
            suggestion: Some("  background: var(--comp-slider-thumb);".into()),
        },
        1,
    );
    let comment = w.review(87).unwrap().line_comments[0].clone();
    assert!(
        w.log
            .last()
            .unwrap()
            .text
            .contains("mit Änderungsvorschlag")
    );
    assert!(w.review(87).unwrap().suggestion_diff(&comment).is_some());
    fails(
        &mut w,
        Action::EditLineComment {
            review: 87,
            comment: comment.id.clone(),
            text: String::new(),
            suggestion: None,
        },
        ActionError::InvalidInput,
    );
    ok(
        &mut w,
        Action::EditLineComment {
            review: 87,
            comment: comment.id.clone(),
            text: "Besser so".into(),
            suggestion: None,
        },
        1,
    );
    assert_eq!(w.review(87).unwrap().line_comments[0].suggestion, None);
}

#[test]
fn new_actions_survive_json_and_old_ones_still_parse() {
    for a in [
        merge(87, MergeMethod::Squash),
        Action::RestartCheck {
            review: 85,
            check: "Lint".into(),
        },
        Action::AdvanceChecks,
    ] {
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    }
    let old = r#"{"AddLineComment":{"review":1,"file":"a","line":0,"text":"x"}}"#;
    assert!(matches!(
        serde_json::from_str::<Action>(old).unwrap(),
        Action::AddLineComment {
            suggestion: None,
            ..
        }
    ));
}

#[test]
fn a_merged_pull_request_survives_saving() {
    let mut w = ws();
    ready_87(&mut w);
    ok(&mut w, merge(87, MergeMethod::Squash), 1);
    let (back, _) = persist::restore(Some(&persist::save(&w)));
    assert_eq!(back, w);
}
