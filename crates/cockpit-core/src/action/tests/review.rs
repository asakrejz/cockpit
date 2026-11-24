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
            review: 87.into(),
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
        ActionError::UnknownReview(1.into()),
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
    // While paused the run takes no step.
    let before = w.run("BAU-150").unwrap().completed;
    advance(&mut w, 2);
    assert_eq!(w.run("BAU-150").unwrap().completed, before);
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
    advance(&mut w, 1);
    assert_eq!(w.run("BAU-150").unwrap().completed, before + 1);
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
    advance(&mut w, STEPS as u64);
    assert!(w.run("BAU-142").unwrap().is_done());
    // In the end nothing is merged: the pull request is created only on your approval.
    assert!(w.reviews.iter().all(|r| r.merged.is_none()));
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
            review: 91.into(),
            comment: String::new(),
        },
        1,
    );
    let applied = ok(&mut w, merge(91, MergeMethod::Rebase), 2);
    assert_eq!(first_text(&w, &applied), "#91 gemergt (Rebase)");
    // The automation updates the ticket, with the system as author.
    let entry = w.log.last().unwrap();
    assert_eq!(entry.actor, Actor::System);
    assert_eq!(entry.target, Some(Target::Item("bau138".into())));
    assert_eq!(
        entry.text,
        "Status von BAU-138 automatisch auf „Fertig“ gesetzt – acme/app#91 ist gemergt"
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
            review: 91.into(),
            verdict: Verdict::Approve,
        },
        ActionError::Merged(PrKey::new("acme/app", 91)),
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
    assert_eq!(item.reason_line().de(), "Gemergt (Squash)");
    let entry = w.log.last().unwrap();
    assert_eq!(
        (entry.actor, &entry.target),
        (Actor::User, &Some(Target::Review(87.into())))
    );
}

#[test]
fn a_failed_check_can_be_restarted_and_runs_until_the_next_sync() {
    let mut w = ws();
    assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
    fails(
        &mut w,
        Action::RestartCheck {
            review: 85.into(),
            check: "Build".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::RestartCheck {
            review: 85.into(),
            check: "Gibt es nicht".into(),
        },
        ActionError::UnknownCheck("Gibt es nicht".into()),
    );
    ok(
        &mut w,
        Action::RestartCheck {
            review: 85.into(),
            check: "Tests (Browser)".into(),
        },
        1,
    );
    let status = |w: &Workspace, name: &str| {
        w.review(85)
            .unwrap()
            .check_runs
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .status
    };
    assert_eq!(status(&w, "Tests (Browser)"), Checks::Running);
    // A running check can not be restarted again.
    fails(
        &mut w,
        Action::RestartCheck {
            review: 85.into(),
            check: "Tests (Browser)".into(),
        },
        ActionError::NothingToDo,
    );
    // The lint error is still red: the overall state stays red.
    assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
    ok(
        &mut w,
        Action::RestartCheck {
            review: 85.into(),
            check: "Lint".into(),
        },
        1,
    );
    // Both restarted: the pull request and its entry show „running“ until the sync reports.
    assert_eq!(w.review(85).unwrap().checks, Checks::Running);
    assert_eq!(
        w.item("pr85").unwrap().pr.as_ref().unwrap().checks,
        Checks::Running
    );
    assert!(w.review(85).unwrap().check_runs[1].log.is_empty());
}

#[test]
fn line_comment_suggestions_go_through_the_actions() {
    let mut w = ws();
    let file = w.review(87).unwrap().files[0].name.clone();
    // Position 8: `  background: #ffffff;` (new line).
    ok(
        &mut w,
        Action::AddLineComment {
            review: 87.into(),
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
            review: 87.into(),
            comment: comment.id.clone(),
            text: String::new(),
            suggestion: None,
        },
        ActionError::InvalidInput,
    );
    ok(
        &mut w,
        Action::EditLineComment {
            review: 87.into(),
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
            review: 85.into(),
            check: "Lint".into(),
        },
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
    let back = loaded(&saved(&w)).unwrap();
    assert_eq!(back, w);
}
