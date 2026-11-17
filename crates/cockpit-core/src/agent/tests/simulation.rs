use super::*;

#[test]
fn a_new_run_is_idle() {
    let r = run();
    assert!(r.is_idle());
    assert!(r.log().starts_with("Noch keine Aktivität"));
    assert!(r.steps().iter().all(|s| s.state == StepState::Pending));
    assert_eq!(r.branch(), "claude/BAU-142-toggle-fokus-ring-fehlt");
}

#[test]
fn advance_only_works_while_running() {
    let mut r = run();
    assert!(!r.advance());
    assert_eq!(r.completed, 0);
}

#[test]
fn exactly_one_step_is_active_while_running() {
    let mut r = quiet();
    r.start(direct(), 92);
    for done in 0..STEPS {
        let states: Vec<_> = r.steps().into_iter().map(|s| s.state).collect();
        assert_eq!(
            states.iter().filter(|s| **s == StepState::Done).count(),
            done
        );
        assert_eq!(
            states.iter().filter(|s| **s == StepState::Active).count(),
            1
        );
        assert_eq!(states[done], StepState::Active);
        assert_eq!(r.log().lines().count(), done + 1);
        let finished = r.advance();
        assert_eq!(finished, done + 1 == STEPS);
    }
    assert!(r.is_done());
    assert!(r.steps().iter().all(|s| s.state == StepState::Done));
    assert!(r.log().ends_with("› Pull Request #92 erstellt (Entwurf)"));
    assert!(!r.advance(), "a finished run stays finished");
    assert_eq!(r.completed, STEPS);
}

#[test]
fn options_show_up_in_steps_and_log() {
    let mut r = quiet();
    let options = AgentOptions {
        run_tests: false,
        draft_pr: false,
        hint: " Auch den Slider prüfen ".into(),
        ..direct()
    };
    r.start(options, 7);
    while !r.advance() {}
    let steps = r.steps();
    assert_eq!(steps[3].title, "Tests übersprungen");
    assert_eq!(steps[4].text, "#7 – wartet auf dein Review");
    let log = r.log();
    assert!(log.contains("› Hinweis: Auch den Slider prüfen\n"), "{log}");
    assert!(log.contains("› Tests übersprungen"));
    assert!(log.contains("› Bearbeite components.css, tokens.css"));
    assert_eq!(
        log.lines().count(),
        STEPS + 1,
        "der Hinweis ist eine Zusatzzeile"
    );
}

#[test]
fn reset_makes_the_run_startable_again() {
    let mut r = run();
    r.start(direct(), 92);
    r.advance();
    r.reset();
    assert!(r.is_idle());
    assert_eq!((r.completed, r.pr_number), (0, None));
}

#[test]
fn a_plan_waits_for_approval() {
    let mut r = run();
    r.options.hint = "Auch den Slider prüfen".into();
    let options = r.options.clone();
    r.start(options, 92);
    assert!(r.is_awaiting() && r.is_active() && !r.is_running());
    assert!(!r.advance(), "no progress without approval");
    assert_eq!(r.completed, 0);
    let plan = r.plan().unwrap();
    assert_eq!(plan.files, ["components.css", "tokens.css"]);
    assert!(plan.summary.contains("Auch den Slider prüfen"));
    assert!(plan.steps.iter().any(|s| s.contains("claude/BAU-142")));
    let steps = r.steps();
    assert_eq!(steps[0].state, StepState::Active);
    assert_eq!(steps[0].title, "Plan liegt vor");
    assert!(steps[1..].iter().all(|s| s.state == StepState::Pending));
    assert!(r.log().ends_with("› Warte auf Freigabe"), "{}", r.log());
    // There is no branch in the log yet.
    assert!(!r.log().contains("git switch"));

    assert!(r.approve());
    assert!(!r.approve());
    assert!(r.log().ends_with("› Plan freigegeben"), "{}", r.log());
    assert!(r.advance() || r.completed == 1);
}

#[test]
fn rejecting_resets_the_run() {
    let mut r = run();
    r.start(AgentOptions::default(), 92);
    assert!(r.revise("noch eine Anmerkung"));
    assert!(r.reject());
    assert!(r.is_idle() && r.plan().is_none());
    assert_eq!(
        (r.pr_number, r.plan_notes.len(), r.planned),
        (None, 0, false)
    );
    assert!(!r.reject());
    assert!(!r.approve());
}

#[test]
fn a_note_creates_a_new_plan() {
    let mut r = run();
    r.start(AgentOptions::default(), 92);
    let before = r.plan().unwrap();
    assert!(!r.revise("   "));
    assert!(r.revise("  Bitte auch Tests für den Slider  "));
    let after = r.plan().unwrap();
    assert_ne!(before, after);
    assert_eq!(after.steps.len(), before.steps.len() + 1);
    assert!(
        after
            .steps
            .iter()
            .any(|s| s == "Berücksichtigen: Bitte auch Tests für den Slider")
    );
    assert!(r.is_awaiting(), "the new plan needs approval again");
    assert!(
        r.log()
            .contains("› Neuer Plan angefordert: Bitte auch Tests für den Slider")
    );
    // After approval it can no longer be revised.
    r.approve();
    assert!(!r.revise("zu spät"));
}

#[test]
fn without_plan_first_the_agent_runs_right_away() {
    let mut r = run();
    r.start(direct(), 92);
    assert!(r.is_running() && r.plan().is_none());
    assert!(!r.approve());
    assert!(!r.log().contains("Plan"));
}
