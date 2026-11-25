use super::*;
use cockpit_i18n::Lang;

fn real_run() -> AgentRun {
    let mut r = run();
    r.start(AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    });
    r.report(
        AgentReport::Started {
            detail: cockpit_i18n::Msg::raw("Container gestartet"),
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
fn the_container_reports_the_files_it_changes() {
    let mut r = real_run();
    r.report(
        AgentReport::Files {
            files: vec!["src/toggle.rs".into()],
        },
        &AgentLimits::unlimited(),
        0,
    );
    assert_eq!(r.files, ["src/toggle.rs"]);
}

#[test]
fn a_followup_keeps_the_files_of_its_comments_when_the_container_starts() {
    let mut r = AgentRun::followup(
        RunKind::Followup {
            review: PrKey::from(91),
            task: FollowupTask::ReviewThreads,
        },
        "BAU-138",
        "Segmented: Pfeiltasten",
        "claude/bau-138",
    );
    r.files = vec!["controls.rs".into()];
    r.start(AgentOptions::default());
    r.report(
        AgentReport::Started {
            detail: cockpit_i18n::Msg::raw("Container gestartet"),
        },
        &AgentLimits::unlimited(),
        0,
    );
    assert_eq!(r.files, ["controls.rs"]);
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
            .text_in(Lang::De)
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
                summary: cockpit_i18n::Msg::raw("Fertig"),
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
                reason: cockpit_i18n::Msg::raw("Der Container endete mit Code 1"),
            },
            &limits,
            0,
        )
        .unwrap();
    assert!(e.failed && r.is_idle());
    assert_eq!(
        r.ended,
        Some(cockpit_i18n::Msg::raw("Der Container endete mit Code 1"))
    );
    assert!(r.log().contains("Der Container endete mit Code 1"));
    // A new start clears the reason.
    r.start(AgentOptions::default());
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
    r.start(AgentOptions::default());
    r.report(
        AgentReport::Started {
            detail: cockpit_i18n::Msg::raw("Plan-Lauf"),
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
    assert_eq!(plan.summary.render(Lang::De), "Fokus-Ring ergänzen");
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

#[test]
fn the_older_german_actor_name_is_still_read_and_the_new_one_written() {
    use crate::Actor;
    for old in ["\"Du\"", "\"User\""] {
        let s: Speaker = serde_json::from_str(old).unwrap();
        assert_eq!(s, Speaker::User, "{old}");
        let a: Actor = serde_json::from_str(old).unwrap();
        assert_eq!(a, Actor::User, "{old}");
    }
    assert_eq!(serde_json::to_string(&Speaker::User).unwrap(), "\"User\"");
    assert_eq!(serde_json::to_string(&Actor::User).unwrap(), "\"User\"");
}
