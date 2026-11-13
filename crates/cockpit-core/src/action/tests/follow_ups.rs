use super::*;

#[test]
fn address_review_pushes_to_the_same_branch_and_resolves_the_comments() {
    let mut w = ws();
    let prs = w.next_pr_number();
    let applied = ok(&mut w, address(91, &["c1", "c2"], direct()), 1);
    assert!(
        applied.summary.contains("2 Kommentare"),
        "{}",
        applied.summary
    );
    let run = w.run("review-asakrejz~rust-playground-91").unwrap();
    assert!(run.is_running() && run.is_followup());
    assert_eq!(run.branch(), "claude/BAU-138-segmented-picker");
    assert_eq!(run.files, ["controls.rs", "components.css"]);
    assert_eq!(w.next_pr_number(), prs, "no new pull request");
    // The ticket with its „implement ticket“ run stays untouched.
    assert!(w.run("BAU-138").unwrap().is_done());

    run_to_end(&mut w);
    let run = w.run("review-asakrejz~rust-playground-91").unwrap();
    assert!(run.is_done());
    let review = w.review(91).unwrap();
    assert!(review.open_comments().is_empty());
    assert!(
        review
            .line_comments
            .iter()
            .all(|c| c.resolved.as_deref() == Some("review-asakrejz~rust-playground-91"))
    );
    // The change suggestion from c1 is now in the diff.
    let file = review
        .files
        .iter()
        .find(|f| f.name == "controls.rs")
        .unwrap();
    assert!(
        file.lines
            .iter()
            .any(|l| l.text.contains("select_with_keys"))
    );
    assert!(!review.sent && review.verdict == Verdict::Comment);
    assert_eq!(w.reviews.len(), 3);
    // The entry shows the new commit and is back at „Review angefragt“.
    let item = w.item_by_key("BAU-138").unwrap();
    assert_eq!(item.reason, crate::model::ReasonKind::ReviewRequested);
    assert!(
        item.agent_note
            .as_deref()
            .unwrap()
            .starts_with("Neuer Commit")
    );
    assert!(!item.pr.as_ref().unwrap().changes_requested);
    assert!(!item.read);
    assert!(item.comments.last().unwrap().author.is_agent());
    // Log: job, steps, result on the review.
    let texts: Vec<&str> = w.log.iter().map(|e| e.text.as_str()).collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Kommentare in asakrejz/rust-playground#91: Branch ausgecheckt"))
    );
    assert!(texts.last().unwrap().contains("auf den Branch gepusht"));
}

#[test]
fn address_review_takes_a_selection_and_rejects_bad_ones() {
    let mut w = ws();
    fails(
        &mut w,
        address(91, &[], direct()),
        ActionError::Agent(AgentError::NoComments),
    );
    fails(
        &mut w,
        address(91, &["x9"], direct()),
        ActionError::UnknownComment("x9".into()),
    );
    fails(
        &mut w,
        address(5, &["c1"], direct()),
        ActionError::UnknownReview(5.into()),
    );
    ok(&mut w, address(91, &["c2"], direct()), 1);
    fails(
        &mut w,
        address(91, &["c1"], direct()),
        ActionError::Agent(AgentError::AlreadyRunning),
    );
    run_to_end(&mut w);
    // Only c2 is done; c1 stays open, and c2 cannot be implemented again.
    let r = w.review(91).unwrap().clone();
    assert_eq!(r.open_comments().len(), 1);
    assert!(r.open_comments()[0].id == "c1");
    fails(
        &mut w,
        address(91, &["c2"], direct()),
        ActionError::Agent(AgentError::NoComments),
    );
    // The result of the first round stayed without c1 being changed.
    assert!(
        r.files
            .iter()
            .all(|f| f.lines.iter().all(|l| !l.text.contains("select_with_keys")))
    );
}

#[test]
fn address_review_waits_for_plan_approval_and_can_be_steered() {
    let mut w = ws();
    ok(&mut w, address(91, &["c1"], AgentOptions::default()), 2);
    let run = w.run("review-asakrejz~rust-playground-91").unwrap();
    assert!(run.is_awaiting());
    assert!(
        run.plan()
            .unwrap()
            .steps
            .iter()
            .any(|s| s.contains("kein neuer Pull Request"))
    );
    assert_eq!(w.waiting_agents(), 1);
    // Nothing happens before approval.
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    assert!(w.review(91).unwrap().open_comments().len() == 2);
    ok(
        &mut w,
        Action::ApprovePlan {
            ticket: "review-asakrejz~rust-playground-91".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::SteerAgent {
            run: "review-asakrejz~rust-playground-91".into(),
            text: "Mit Tests".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::PauseAgent {
            run: "review-asakrejz~rust-playground-91".into(),
        },
        1,
    );
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    ok(
        &mut w,
        Action::ResumeAgent {
            run: "review-asakrejz~rust-playground-91".into(),
        },
        1,
    );
    // Follow-up question after the second step, with the answer it continues.
    // The first tick additionally acknowledges the hint.
    ok(&mut w, Action::AdvanceAgents, 2);
    ok(&mut w, Action::AdvanceAgents, 1);
    let step = w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
    assert!(step.logged >= 1);
    assert!(
        w.run("review-asakrejz~rust-playground-91")
            .unwrap()
            .is_asking()
    );
    ok(
        &mut w,
        Action::AnswerAgent {
            run: "review-asakrejz~rust-playground-91".into(),
            text: "Nein".into(),
        },
        1,
    );
    run_to_end(&mut w);
    assert!(
        w.run("review-asakrejz~rust-playground-91")
            .unwrap()
            .is_done()
    );
    // Rejecting removes the follow-up job completely.
    let mut w = ws();
    ok(&mut w, address(91, &["c1"], AgentOptions::default()), 2);
    ok(
        &mut w,
        Action::RejectPlan {
            ticket: "review-asakrejz~rust-playground-91".into(),
        },
        1,
    );
    assert!(w.run("review-asakrejz~rust-playground-91").is_none());
    assert_eq!(w.review(91).unwrap().open_comments().len(), 2);
}

#[test]
fn foreign_comments_stay_out_of_my_review_and_cannot_be_edited() {
    let mut w = ws();
    let r = w.review(91).unwrap();
    assert_eq!(
        r.comment_count(),
        0,
        "fremde Kommentare gehen nicht mit hinaus"
    );
    fails(
        &mut w,
        Action::EditLineComment {
            review: 91.into(),
            comment: "c1".into(),
            text: "Anders".into(),
            suggestion: None,
        },
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        Action::DeleteLineComment {
            review: 91.into(),
            comment: "c1".into(),
        },
        ActionError::UnknownComment("c1".into()),
    );
    // Own, implemented comments do not go out again.
    let mut r = w.review(87).unwrap().clone();
    let id = r
        .add_line_comment("tokens.css", 1, "Eigener", None)
        .unwrap();
    assert_eq!(r.comment_count(), 1);
    r.resolve_comments(&[id], "review-87");
    assert_eq!(r.comment_count(), 0);
}

#[test]
fn a_red_check_is_diagnosed_and_nothing_changes_before_you_accept() {
    let mut w = ws();
    investigate_85(&mut w, "Lint");
    let key = "check-asakrejz~rust-playground-85-lint";
    assert!(w.run(key).unwrap().is_running());
    let branch_before = w.review(85).unwrap().files.clone();
    run_to_end(&mut w);
    let run = w.run(key).unwrap();
    assert!(run.is_diagnosed(), "waits for the decision");
    let d = run.diagnosis.clone().unwrap();
    assert_eq!(d.location().as_deref(), Some("list.rs · Zeile 62"));
    assert!(d.patch.is_some());
    assert_eq!(
        w.review(85).unwrap().files,
        branch_before,
        "Branch unberührt"
    );
    assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
    assert_eq!(w.waiting_agents(), 1);
    // It does not continue on its own.
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    // Attention: the diagnosis is at the top under „hängt“.
    let stuck = crate::team::stuck(&w);
    let entry = stuck.iter().find(|s| s.key == "#85").unwrap();
    assert_eq!(entry.kind, crate::team::StuckKind::DiagnosisWaiting);
    assert_eq!(entry.diagnosis.as_deref(), Some(key));

    ok(&mut w, Action::ApplyFix { run: key.into() }, 1);
    run_to_end(&mut w);
    assert!(w.run(key).unwrap().is_done());
    let review = w.review(85).unwrap();
    let list = review.files.iter().find(|f| f.name == "list.rs").unwrap();
    assert!(list.lines.iter().all(|l| !l.text.contains("let inset")));
    let lint = review.check_runs.iter().find(|c| c.name == "Lint").unwrap();
    assert_eq!(lint.status, Checks::Running, "Check neu gestartet");
    // The tick of the checks turns it green; the browser test stays red.
    while w.running_checks() > 0 {
        w.apply(Action::AdvanceChecks, Actor::System, T).unwrap();
    }
    let review = w.review(85).unwrap();
    assert_eq!(
        review
            .check_runs
            .iter()
            .find(|c| c.name == "Lint")
            .unwrap()
            .status,
        Checks::Passed
    );
    assert_eq!(
        review.checks,
        Checks::Failed,
        "Tests (Browser) ist weiter rot"
    );
    assert!(
        w.item_by_key("#85")
            .unwrap()
            .comments
            .last()
            .unwrap()
            .text
            .contains("gepusht")
    );
}

#[test]
fn discarding_a_diagnosis_ends_the_run_without_touching_the_branch() {
    let mut w = ws();
    investigate_85(&mut w, "Lint");
    run_to_end(&mut w);
    let before = w.review(85).unwrap().clone();
    ok(
        &mut w,
        Action::DiscardFix {
            run: "check-asakrejz~rust-playground-85-lint".into(),
        },
        1,
    );
    assert!(w.run("check-asakrejz~rust-playground-85-lint").is_none());
    assert_eq!(*w.review(85).unwrap(), before);
    fails(
        &mut w,
        Action::ApplyFix {
            run: "check-asakrejz~rust-playground-85-lint".into(),
        },
        ActionError::NothingToDo,
    );
    // After that one can investigate again.
    investigate_85(&mut w, "Lint");
}

#[test]
fn a_flaky_check_is_only_restarted_and_turns_green() {
    let mut w = ws();
    investigate_85(&mut w, "Tests (Browser)");
    let key = "check-asakrejz~rust-playground-85-tests-browser";
    run_to_end(&mut w);
    assert!(!w.run(key).unwrap().diagnosis.unwrap().changes_code());
    let files = w.review(85).unwrap().files.clone();
    ok(&mut w, Action::ApplyFix { run: key.into() }, 1);
    run_to_end(&mut w);
    assert_eq!(w.review(85).unwrap().files, files);
    while w.running_checks() > 0 {
        w.apply(Action::AdvanceChecks, Actor::System, T).unwrap();
    }
    let r = w.review(85).unwrap();
    assert_eq!(
        r.check_runs
            .iter()
            .find(|c| c.name == "Tests (Browser)")
            .unwrap()
            .status,
        Checks::Passed
    );
}

#[test]
fn only_a_red_check_can_be_investigated() {
    let mut w = ws();
    fails(
        &mut w,
        Action::InvestigateCheck {
            review: 85.into(),
            check: "Build".into(),
        },
        ActionError::Agent(AgentError::CheckNotFailed),
    );
    fails(
        &mut w,
        Action::InvestigateCheck {
            review: 85.into(),
            check: "Nope".into(),
        },
        ActionError::UnknownCheck("Nope".into()),
    );
    fails(
        &mut w,
        Action::InvestigateCheck {
            review: 9.into(),
            check: "Lint".into(),
        },
        ActionError::UnknownReview(9.into()),
    );
    investigate_85(&mut w, "Lint");
    fails(
        &mut w,
        Action::InvestigateCheck {
            review: 85.into(),
            check: "Lint".into(),
        },
        ActionError::Agent(AgentError::AlreadyRunning),
    );
}

#[test]
fn followups_notify_when_waiting_and_when_done() {
    use crate::notify::{NotifyReason, events};
    let mut w = ws();
    investigate_85(&mut w, "Lint");
    let mut waiting = None;
    let mut before = w.clone();
    for _ in 0..STEPS {
        w.apply(Action::AdvanceAgents, Actor::Agent, T).ok();
        let e = events(&before, &w);
        if !e.is_empty() {
            waiting = Some(e);
            break;
        }
        before = w.clone();
    }
    let e = waiting.unwrap();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].reason, NotifyReason::AgentWaiting);
    assert!(
        e[0].title
            .contains("Check „Lint“ in asakrejz/rust-playground#85")
    );
    assert_eq!(e[0].route, "#/agent/check-asakrejz~rust-playground-85-lint");
    ok(
        &mut w,
        Action::ApplyFix {
            run: "check-asakrejz~rust-playground-85-lint".into(),
        },
        1,
    );
    let mut before = w.clone();
    let mut done = Vec::new();
    for _ in 0..STEPS {
        w.apply(Action::AdvanceAgents, Actor::Agent, T).ok();
        done.extend(events(&before, &w));
        before = w.clone();
    }
    let finished: Vec<_> = done
        .iter()
        .filter(|e| e.reason == NotifyReason::AgentFinished)
        .collect();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].route, "#/review/asakrejz/rust-playground/85");
}

#[test]
fn nobody_but_you_merges_after_followups_and_followups_count_for_the_team() {
    let mut w = ws();
    ok(&mut w, address(91, &["c1"], direct()), 1);
    let team = crate::team::members(&w);
    let agent = team.iter().find(|m| m.person.is_agent()).unwrap();
    assert_eq!(
        (agent.in_progress, agent.review),
        (1, 0),
        "BAU-138 zählt einmal, jetzt in Arbeit"
    );
    assert_eq!(agent.item_ids, ["bau138"]);
    run_to_end(&mut w);
    assert_eq!(
        w.apply(
            Action::MergePr {
                review: 91.into(),
                method: MergeMethod::Squash
            },
            Actor::Agent,
            T
        ),
        Err(ActionError::MergeNotAllowed(Actor::Agent))
    );
}

#[test]
fn followup_actions_and_runs_survive_json_and_old_saves() {
    let a = address(91, &["c1"], direct());
    let json = serde_json::to_string(&a).unwrap();
    assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    for a in [
        Action::InvestigateCheck {
            review: 85.into(),
            check: "Lint".into(),
        },
        Action::ApplyFix {
            run: "check-asakrejz~rust-playground-85-lint".into(),
        },
        Action::DiscardFix {
            run: "check-asakrejz~rust-playground-85-lint".into(),
        },
    ] {
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    }
    // Save and load in the middle of the diagnosis.
    let mut w = ws();
    investigate_85(&mut w, "Lint");
    run_to_end(&mut w);
    let (back, _) = persist::restore(Some(&persist::save(&w)));
    assert_eq!(back, w);
    assert!(
        back.run("check-asakrejz~rust-playground-85-lint")
            .unwrap()
            .is_diagnosed()
    );
    // Old runs without `kind` are „implement ticket“.
    let old: crate::agent::AgentRun =
        serde_json::from_str(r#"{"ticket_key":"BAU-1","state":"Done"}"#).unwrap();
    assert_eq!(old.kind, crate::agent::RunKind::Implement);
    assert_eq!(old.key(), "BAU-1");
    // Old comments without an author belong to me and are open.
    let c: crate::review::LineComment =
        serde_json::from_str(r#"{"id":"c1","file":"a","line":0,"text":"x"}"#).unwrap();
    assert!(c.is_own() && c.is_open());
}
