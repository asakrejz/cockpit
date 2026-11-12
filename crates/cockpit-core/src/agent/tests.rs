use super::*;

/// Without plan approval: the run starts immediately.
fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..Default::default()
    }
}

/// A run without a follow-up question (the demo asks only for BAU-142).
fn quiet() -> AgentRun {
    let mut r = run();
    r.ticket_key = "BAU-200".into();
    r
}

fn run() -> AgentRun {
    let mut r = AgentRun::new("BAU-142", "Toggle: Fokus-Ring fehlt im Dark Mode");
    r.comments = 1;
    r.files = vec!["components.css".into(), "tokens.css".into()];
    r
}

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

/// Brings BAU-142 up to the follow-up question.
fn asking() -> AgentRun {
    let mut r = run();
    r.start(direct(), 92);
    assert_eq!(r.tick(), Tick::Stepped);
    assert_eq!(r.tick(), Tick::Stepped);
    assert_eq!(r.tick(), Tick::Asked);
    r
}

#[test]
fn the_agent_asks_a_question_and_waits() {
    let mut r = asking();
    assert!(r.is_asking() && r.is_active() && !r.is_running());
    assert_eq!(r.completed, 2, "the question is not a step");
    let q = r.question.clone().unwrap();
    assert!(q.text.contains("deprecated"));
    assert_eq!(q.suggestions.len(), 3);
    assert_eq!(r.tick(), Tick::Idle, "no progress without an answer");
    assert!(!r.advance());
    assert_eq!(r.completed, 2);
    let steps = r.steps();
    assert_eq!(steps[2].title, "Frage an dich");
    assert_eq!(steps[2].state, StepState::Active);
    assert!(steps[3..].iter().all(|s| s.state == StepState::Pending));
    assert!(
        r.log().contains("› Frage: Soll der alte Stil"),
        "{}",
        r.log()
    );
    assert!(r.log().ends_with("› Warte auf deine Antwort"));
}

#[test]
fn an_answer_continues_the_run_and_stays_in_the_history() {
    let mut r = asking();
    assert!(!r.answer("   "), "empty does not count");
    assert!(r.is_asking());
    assert!(r.answer("  Als deprecated markieren "));
    assert!(r.is_running() && r.question.is_none());
    assert!(!r.answer("noch einmal"), "answer only once");
    let last = r.messages.last().unwrap();
    assert_eq!(
        (last.from, last.kind, last.text.as_str()),
        (
            Speaker::User,
            MessageKind::Answer,
            "Als deprecated markieren"
        )
    );
    assert!(r.log().contains("› Antwort: Als deprecated markieren"));
    // The question comes only once; the run goes to the end.
    while !r.advance() {}
    assert!(r.is_done());
    assert_eq!(
        r.messages
            .iter()
            .filter(|m| m.kind == MessageKind::Question)
            .count(),
        1
    );
    assert!(
        r.steps()[2]
            .text
            .ends_with("Deine Antwort: Als deprecated markieren")
    );
}

#[test]
fn other_tickets_ask_nothing() {
    let mut r = quiet();
    r.start(direct(), 92);
    while !r.advance() {}
    assert!(r.messages.iter().all(|m| m.kind != MessageKind::Question));
}

#[test]
fn a_hint_is_taken_into_account_at_the_next_step() {
    let mut r = quiet();
    r.start(direct(), 92);
    assert!(!r.steer("  "), "empty does not count");
    assert!(r.steer(" Auch den Slider prüfen "));
    assert_eq!(r.pending_hints, ["Auch den Slider prüfen"]);
    assert!(
        !r.messages.iter().any(|m| m.kind == MessageKind::Ack),
        "noch nicht berücksichtigt"
    );
    assert_eq!(r.tick(), Tick::Stepped);
    assert!(r.pending_hints.is_empty());
    let ack = r
        .messages
        .iter()
        .find(|m| m.kind == MessageKind::Ack)
        .unwrap();
    assert_eq!(
        (ack.from, ack.text.as_str()),
        (Speaker::Agent, "Hinweis erhalten: Auch den Slider prüfen")
    );
    assert!(
        r.log()
            .contains("› Hinweis erhalten: Auch den Slider prüfen")
    );
}

#[test]
fn hints_only_work_while_the_agent_works() {
    let mut r = quiet();
    assert!(!r.steer("too early"), "idle");
    r.start(AgentOptions::default(), 92);
    assert!(
        !r.steer("Plan noch offen"),
        "dafür gibt es die Plan-Anmerkung"
    );
    r.approve();
    assert!(r.steer("jetzt"));
    while !r.advance() {}
    assert!(!r.steer("zu spät"), "done");
}

#[test]
fn pause_and_resume() {
    let mut r = quiet();
    assert!(!r.pause() && !r.resume());
    r.start(direct(), 92);
    r.advance();
    assert!(r.pause() && r.is_paused() && r.is_active());
    assert!(!r.pause(), "schon pausiert");
    assert_eq!(r.tick(), Tick::Idle);
    assert_eq!(r.completed, 1, "paused means: no progress");
    assert_eq!(r.steps()[1].state, StepState::Active);
    assert!(r.steps()[1].text.starts_with("Pausiert"));
    assert!(r.log().ends_with("› Pausiert"));
    // Hints can also be sent while paused and wait until resume.
    assert!(r.steer("Während der Pause"));
    assert_eq!(r.tick(), Tick::Idle);
    assert_eq!(r.pending_hints.len(), 1);
    assert!(r.resume() && r.is_running() && !r.resume());
    assert_eq!(r.tick(), Tick::Stepped);
    assert!(r.pending_hints.is_empty());
}

#[test]
fn a_question_cannot_be_skipped_by_pausing() {
    let mut r = asking();
    assert!(!r.pause(), "whoever asks is not running");
    assert!(r.is_asking());
}

#[test]
fn reset_clears_the_conversation() {
    let mut r = asking();
    r.steer("noch ein Hinweis");
    r.reset();
    assert!(r.messages.is_empty() && r.question.is_none() && r.pending_hints.is_empty());
    // A new run asks again.
    r.start(direct(), 93);
    assert_eq!(r.messages.len(), 1);
    r.advance();
    r.advance();
    assert_eq!(r.tick(), Tick::Asked);
}

#[test]
fn the_history_tells_the_story() {
    let mut r = run();
    r.start(
        AgentOptions {
            hint: "Bitte vorsichtig".into(),
            ..AgentOptions::default()
        },
        92,
    );
    r.revise("Auch Tests");
    r.approve();
    let texts: Vec<_> = r.messages.iter().map(|m| (m.from, m.kind)).collect();
    assert_eq!(
        texts,
        [
            (Speaker::Agent, MessageKind::Info),
            (Speaker::User, MessageKind::Hint),
            (Speaker::Agent, MessageKind::Info),
            (Speaker::User, MessageKind::Hint),
            (Speaker::User, MessageKind::Info),
        ]
    );
}

fn real_run() -> AgentRun {
    let mut r = run();
    r.start(
        AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        },
        92,
    );
    r.report(
        AgentReport::Started {
            detail: "Container gestartet".into(),
        },
        &AgentLimits::unlimited(),
        0,
    );
    r
}

fn step(tokens_in: u64, tokens_out: u64, millis: u64) -> AgentReport {
    AgentReport::Step {
        title: "Datei lesen".into(),
        text: "src/lib.rs".into(),
        input_tokens: tokens_in,
        output_tokens: tokens_out,
        millis,
    }
}

#[test]
fn a_real_run_is_not_touched_by_the_simulated_tick() {
    let mut r = real_run();
    assert!(r.real);
    assert_eq!(r.tick(), Tick::Idle);
    assert_eq!(r.completed, 0);
}

#[test]
fn steps_record_usage_with_the_prices_and_never_finish_the_run() {
    let mut r = real_run();
    let limits = AgentLimits::unlimited();
    for _ in 0..10 {
        let e = r.report(step(1_000_000, 0, 1_000), &limits, 0).unwrap();
        assert_eq!(e.usage.unwrap().cost_cents, 300);
        assert!(!e.finished);
    }
    assert_eq!(r.completed, STEPS - 1);
    assert!(r.is_running());
    assert_eq!(r.usage().cost_cents, 3_000);
    assert_eq!(r.usage().millis, 10_000);
}

#[test]
fn the_budget_stops_a_real_run_with_a_plain_message() {
    let mut r = real_run();
    let limits = AgentLimits {
        run_budget_cents: Some(100),
        ..AgentLimits::unlimited()
    };
    let e = r.report(step(1_000_000, 0, 10), &limits, 0).unwrap();
    assert_eq!(e.stopped, Some(StopReason::RunBudget));
    assert_eq!(r.stop_reason(), Some(StopReason::RunBudget));
    assert!(
        r.messages
            .last()
            .unwrap()
            .text
            .starts_with("Angehalten: Budget pro Lauf erreicht")
    );
}

#[test]
fn the_month_budget_counts_what_the_step_just_cost() {
    let mut r = real_run();
    let limits = AgentLimits {
        month_budget_cents: Some(400),
        ..AgentLimits::unlimited()
    };
    let e = r.report(step(1_000_000, 0, 10), &limits, 150).unwrap();
    assert_eq!(e.stopped, Some(StopReason::MonthBudget));
}

#[test]
fn a_question_waits_for_the_answer_and_the_answer_resumes() {
    let mut r = real_run();
    let limits = AgentLimits::unlimited();
    let e = r
        .report(
            AgentReport::Question {
                text: "Welcher Branch?".into(),
                suggestions: vec!["main".into()],
            },
            &limits,
            0,
        )
        .unwrap();
    assert!(e.asking && r.is_asking());
    assert_eq!(r.question.as_ref().unwrap().suggestions, ["main"]);
    assert!(r.answer("main"));
    assert!(r.is_running());
}

#[test]
fn hints_are_acknowledged_when_the_container_got_them() {
    let mut r = real_run();
    r.steer("Bitte kleine Commits");
    assert_eq!(r.pending_hints.len(), 1);
    r.report(
        AgentReport::HintsDelivered { count: 1 },
        &AgentLimits::unlimited(),
        0,
    );
    assert!(r.pending_hints.is_empty());
    assert_eq!(r.messages.last().unwrap().kind, MessageKind::Ack);
}

#[test]
fn done_finishes_and_failed_resets_with_the_reason() {
    let mut r = real_run();
    let limits = AgentLimits::unlimited();
    let e = r
        .report(
            AgentReport::Done {
                summary: "Fertig".into(),
            },
            &limits,
            0,
        )
        .unwrap();
    assert!(e.finished && r.is_done() && r.completed == STEPS);

    let mut r = real_run();
    let e = r
        .report(
            AgentReport::Failed {
                reason: "Der Container endete mit Code 1".into(),
            },
            &limits,
            0,
        )
        .unwrap();
    assert!(e.failed && r.is_idle() && !r.real);
    assert_eq!(r.ended.as_deref(), Some("Der Container endete mit Code 1"));
    assert!(r.log().contains("Der Container endete mit Code 1"));
    // A new start clears the reason.
    r.start(AgentOptions::default(), 92);
    assert_eq!(r.ended, None);
}

#[test]
fn reports_for_an_inactive_run_change_nothing() {
    let mut r = run();
    let before = r.clone();
    assert!(
        r.report(step(1, 1, 1), &AgentLimits::unlimited(), 0)
            .is_none()
    );
    assert_eq!(r, before);
}

#[test]
fn the_plan_comes_from_the_agent_and_cannot_be_approved_before() {
    let mut r = run();
    let limits = AgentLimits::unlimited();
    r.start(AgentOptions::default(), 92);
    r.report(
        AgentReport::Started {
            detail: "Plan-Lauf".into(),
        },
        &limits,
        0,
    );
    assert!(r.is_drafting());
    assert!(!r.approve(), "without a plan there is nothing to approve");
    r.report(
        AgentReport::Plan {
            summary: "Fokus-Ring ergänzen".into(),
            files: vec!["src/toggle.rs".into()],
            steps: vec!["Lesen".into(), "Ändern".into()],
        },
        &limits,
        0,
    );
    let plan = r.plan().unwrap();
    assert_eq!(plan.summary, "Fokus-Ring ergänzen");
    assert_eq!(r.files, ["src/toggle.rs"]);
    assert!(!r.is_drafting());
    // A note demands a new plan.
    assert!(r.revise("Auch Tests"));
    assert!(r.is_drafting() && !r.approve());
    r.report(
        AgentReport::Plan {
            summary: "Neu".into(),
            files: vec![],
            steps: vec![],
        },
        &limits,
        0,
    );
    assert!(r.approve());
}

#[test]
fn a_plan_report_outside_the_approval_is_ignored() {
    let mut r = real_run();
    assert!(
        r.report(
            AgentReport::Plan {
                summary: "x".into(),
                files: vec![],
                steps: vec![]
            },
            &AgentLimits::unlimited(),
            0
        )
        .is_none()
    );
}
