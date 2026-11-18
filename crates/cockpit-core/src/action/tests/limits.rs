use super::*;

#[test]
fn every_step_has_usage_and_lands_in_the_ledger() {
    let mut w = ws();
    start(&mut w, "BAU-150");
    advance(&mut w, STEPS as u64);
    let run = w.run("BAU-150").unwrap();
    assert!(run.is_done());
    assert_eq!(run.step_usage.len(), STEPS);
    let total = run.usage();
    assert!(total.input_tokens > 0 && total.output_tokens > 0);
    assert!(total.cost_cents > 0 && total.millis > 0);
    assert_eq!(w.spent_today(T), total);
    assert_eq!(w.spent_month(T), total);
    // Every step shows its consumption.
    assert!(run.steps().iter().all(|s| s.usage.is_some()));
    // The same inputs, the same values.
    let mut w2 = ws();
    start(&mut w2, "BAU-150");
    advance(&mut w2, STEPS as u64);
    assert_eq!(w2.run("BAU-150").unwrap().usage(), total);
}

#[test]
fn prices_are_a_setting_and_cost_follows_them() {
    let mut free = ws();
    ok(
        &mut free,
        limits(|l| {
            l.pricing.input_cents_per_mtok = 0;
            l.pricing.output_cents_per_mtok = 0;
        }),
        2,
    );
    start(&mut free, "BAU-150");
    advance(&mut free, STEPS as u64);
    let run = free.run("BAU-150").unwrap();
    assert_eq!(run.usage().cost_cents, 0);
    assert!(run.usage().tokens() > 0);
}

#[test]
fn the_run_budget_stops_the_run_and_you_can_raise_it() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
    start(&mut w, "BAU-150");
    advance(&mut w, 10);
    let run = w.run("BAU-150").unwrap();
    assert_eq!(run.state, RunState::Stopped(StopReason::RunBudget));
    assert!(run.completed < STEPS);
    assert!(run.usage().cost_cents >= 20);
    assert_eq!(w.stopped_agents(), 1);
    assert_eq!(w.waiting_agents(), 1);
    assert!(
        w.log
            .iter()
            .any(|e| e.text == "BAU-150: Angehalten – Budget pro Lauf erreicht")
    );
    // A stopped run makes no progress.
    let done = run.completed;
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    assert_eq!(w.run("BAU-150").unwrap().completed, done);
    // Without a raise the limit stays reached, and nothing changes.
    fails(
        &mut w,
        Action::ResumeStopped {
            run: "BAU-150".into(),
            extra: 0,
        },
        ActionError::LimitStillReached(StopReason::RunBudget),
    );
    let applied = ok(
        &mut w,
        Action::ResumeStopped {
            run: "BAU-150".into(),
            extra: 500,
        },
        1,
    );
    assert_eq!(
        applied.summary,
        "Agent für BAU-150 fortgesetzt – das Budget dieses Laufs um 5,00 € erhöht"
    );
    // Only this run gets more, the setting stays.
    assert_eq!(w.limits.run_budget_cents, Some(20));
    advance(&mut w, 10);
    assert!(w.run("BAU-150").unwrap().is_done());
}

#[test]
fn the_time_limit_stops_the_run() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.run_minutes = Some(1)), 1);
    start(&mut w, "BAU-150");
    advance(&mut w, 10);
    let run = w.run("BAU-150").unwrap();
    assert_eq!(run.state, RunState::Stopped(StopReason::Time));
    ok(
        &mut w,
        Action::ResumeStopped {
            run: "BAU-150".into(),
            extra: 30,
        },
        1,
    );
    advance(&mut w, 10);
    assert!(w.run("BAU-150").unwrap().is_done());
}

#[test]
fn the_month_budget_covers_all_runs_and_is_raised_in_the_settings() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.month_budget_cents = Some(30)), 1);
    start(&mut w, "BAU-150");
    advance(&mut w, 10);
    assert_eq!(
        w.run("BAU-150").unwrap().state,
        RunState::Stopped(StopReason::MonthBudget)
    );
    // A second run stops immediately: the monthly budget is already used up.
    start(&mut w, "BAU-142");
    advance(&mut w, 1);
    assert_eq!(
        w.run("BAU-142").unwrap().state,
        RunState::Stopped(StopReason::MonthBudget)
    );
    ok(
        &mut w,
        Action::ResumeStopped {
            run: "BAU-150".into(),
            extra: 10_000,
        },
        1,
    );
    assert_eq!(w.limits.month_budget_cents, Some(10_030));
    // The other run continues afterwards without a further raise.
    ok(
        &mut w,
        Action::ResumeStopped {
            run: "BAU-142".into(),
            extra: 0,
        },
        1,
    );
}

#[test]
fn a_stopped_run_can_be_cancelled_and_its_cost_stays_booked() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
    start(&mut w, "BAU-150");
    advance(&mut w, 10);
    let spent = w.spent_month(T);
    assert!(spent.cost_cents >= 20);
    ok(
        &mut w,
        Action::CancelAgent {
            ticket: "BAU-150".into(),
        },
        1,
    );
    assert!(w.run("BAU-150").unwrap().is_idle());
    assert_eq!(w.spent_month(T), spent, "costs are not lost");
    assert!(w.run("BAU-150").unwrap().step_usage.is_empty());
}

#[test]
fn a_stopped_run_needs_attention_and_notifies() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
    start(&mut w, "BAU-150");
    let mut before = w.clone();
    for i in 0..10 {
        if w.run("BAU-150").unwrap().is_stopped() {
            break;
        }
        before = w.clone();
        let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T + i);
    }
    let events = crate::notify::events(&before, &w);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].reason, crate::NotifyReason::AgentStopped);
    assert_eq!(events[0].route, "#/agent/BAU-150");
    assert!(events[0].text.de().contains("Budget pro Lauf erreicht"));
    let stuck = crate::team::stuck(&w);
    let entry = stuck.iter().find(|s| s.key == "BAU-150").unwrap();
    assert_eq!(entry.kind, crate::StuckKind::AgentStopped);
    // Report only once.
    assert!(crate::notify::events(&w, &w).is_empty());
}

#[test]
fn too_many_parallel_runs_do_not_start() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.max_parallel = 1), 1);
    start(&mut w, "BAU-150");
    fails(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        ActionError::ParallelLimit(1),
    );
    assert!(
        ActionError::ParallelLimit(1)
            .message()
            .starts_with("Es arbeiten schon 1 Agenten")
    );
    // A run with a plan only waits and occupies no slot – only the approval counts.
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        },
        2,
    );
    fails(
        &mut w,
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        ActionError::ParallelLimit(1),
    );
    // Pausing frees the slot; resuming needs it again.
    ok(
        &mut w,
        Action::PauseAgent {
            run: "BAU-150".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        1,
    );
    fails(
        &mut w,
        Action::ResumeAgent {
            run: "BAU-150".into(),
        },
        ActionError::ParallelLimit(1),
    );
}

#[test]
fn limits_are_validated_and_logged() {
    let mut w = ws();
    fails(
        &mut w,
        limits(|l| l.max_parallel = 0),
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        limits(|l| l.run_budget_cents = Some(0)),
        ActionError::InvalidInput,
    );
    fails(&mut w, limits(|_| {}), ActionError::Unchanged);
    let applied = ok(&mut w, limits(|l| l.max_parallel = 2), 1);
    assert!(applied.summary.contains("höchstens 2 parallel"));
    assert_eq!(w.log.last().unwrap().target, Some(Target::Settings));
}

#[test]
fn a_plan_touching_forbidden_paths_cannot_be_approved() {
    let mut w = ws();
    ok(
        &mut w,
        rules(|r| r.forbidden_paths = vec!["tokens.css".into()]),
        1,
    );
    // Even without a requested plan the run waits for approval.
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        2,
    );
    let run = w.run("BAU-142").unwrap();
    assert!(run.is_awaiting());
    let plan = run.plan().unwrap();
    assert_eq!(plan.violations.len(), 1);
    assert_eq!(plan.violations[0].file, "tokens.css");
    assert_eq!(plan.rules, ["Verboten: tokens.css"]);
    let err = w
        .apply(
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            Actor::User,
            T,
        )
        .unwrap_err();
    assert!(matches!(err, ActionError::PlanViolatesRules(_)));
    assert!(err.message().contains("tokens.css ist verboten"));
    assert!(w.run("BAU-142").unwrap().is_awaiting());
    // The state model itself does not release it either.
    assert!(!w.approve_plan("BAU-142"));
    // If the rule is taken back, that applies to the waiting plan immediately.
    ok(&mut w, rules(|_| {}), 1);
    ok(
        &mut w,
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        1,
    );
    assert!(w.run("BAU-142").unwrap().is_running());
}

#[test]
fn allowed_paths_limit_what_the_plan_may_touch() {
    let mut w = ws();
    ok(&mut w, rules(|r| r.allowed_paths = vec!["*.rs".into()]), 1);
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        },
        2,
    );
    let plan = w.run("BAU-142").unwrap().plan().unwrap();
    assert_eq!(plan.violations.len(), 2);
    assert!(
        plan.violations[0]
            .reason()
            .contains("außerhalb der erlaubten")
    );
}

#[test]
fn required_tests_and_plan_approval_are_enforced_at_the_start() {
    let mut w = ws();
    ok(
        &mut w,
        rules(|r| {
            r.required_tests = vec!["cargo test".into(), "cargo clippy".into()];
            r.plan_required = true;
            r.instructions = "Kleine Commits.".into();
        }),
        1,
    );
    let off = AgentOptions {
        run_tests: false,
        plan_first: false,
        ..AgentOptions::default()
    };
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: off,
        },
        2,
    );
    let run = w.run("BAU-150").unwrap();
    assert!(run.options.run_tests && run.options.plan_first);
    assert!(run.is_awaiting());
    let plan = run.plan().unwrap();
    assert!(plan.violations.is_empty());
    assert!(plan.rules.iter().any(|l| l.contains("Pflicht-Tests")));
    assert!(plan.rules.iter().any(|l| l == "Anweisung: Kleine Commits."));
    assert!(
        plan.steps
            .iter()
            .any(|s| s == "Pflicht-Tests ausführen: cargo test, cargo clippy")
    );
    ok(
        &mut w,
        Action::ApprovePlan {
            ticket: "BAU-150".into(),
        },
        1,
    );
    advance(&mut w, 10);
    let run = w.run("BAU-150").unwrap();
    let steps = run.steps();
    assert_eq!(steps[3].title, "Pflicht-Tests ausgeführt");
    assert!(run.log().contains("$ cargo clippy … bestanden"));
}

#[test]
fn rules_are_per_repo_cleaned_and_validated() {
    let mut w = ws();
    fails(
        &mut w,
        Action::SetRepoRules {
            repo: "kein repo".into(),
            rules: RepoRules::default(),
        },
        ActionError::UnknownRepo("kein repo".into()),
    );
    ok(
        &mut w,
        rules(|r| r.forbidden_paths = vec!["  a/** ".into(), "".into()]),
        1,
    );
    assert_eq!(
        w.rules_for(crate::rules::DEMO_REPO).forbidden_paths,
        ["a/**"]
    );
    fails(
        &mut w,
        rules(|r| r.forbidden_paths = vec!["a/**".into()]),
        ActionError::Unchanged,
    );
    let applied = ok(&mut w, rules(|_| {}), 1);
    assert!(applied.summary.ends_with("entfernt"));
}

#[test]
fn settings_survive_the_demo_reset_but_the_ledger_does_not() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.max_parallel = 2), 1);
    start(&mut w, "BAU-150");
    advance(&mut w, 2);
    assert!(!w.spend.is_empty());
    w.apply(Action::ResetDemo, Actor::User, T).unwrap();
    assert_eq!(w.limits.max_parallel, 2);
    assert!(w.spend.is_empty());
    assert!(!w.repo_rules.is_empty());
}

#[test]
fn the_new_actions_roundtrip_and_old_saves_still_load() {
    for a in [
        Action::ResumeStopped {
            run: "BAU-1".into(),
            extra: 100,
        },
        limits(|l| l.max_parallel = 5),
        rules(|r| r.plan_required = true),
    ] {
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    }
    // A state without the new fields loads with default values.
    let mut w = ws();
    start(&mut w, "BAU-150");
    let mut v: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
    let ws_obj = v["workspace"].as_object_mut().unwrap();
    for key in ["limits", "spend", "repo_rules"] {
        ws_obj.remove(key);
    }
    for run in ws_obj["runs"].as_array_mut().unwrap() {
        let run = run.as_object_mut().unwrap();
        for key in [
            "repo",
            "rules",
            "step_usage",
            "extra_budget_cents",
            "extra_minutes",
        ] {
            run.remove(key);
        }
    }
    let (back, restored) = persist::restore(Some(&v.to_string()));
    assert!(restored);
    assert_eq!(back.limits, AgentLimits::default());
    assert!(back.spend.is_empty());
    assert!(back.run("BAU-150").unwrap().is_running());
    // The stopped state survives saving.
    let mut s = ws();
    ok(&mut s, limits(|l| l.run_budget_cents = Some(20)), 1);
    start(&mut s, "BAU-150");
    advance(&mut s, 10);
    let (back, _) = persist::restore(Some(&persist::save(&s)));
    assert_eq!(back, s);
    assert!(back.run("BAU-150").unwrap().is_stopped());
}

#[test]
fn the_agent_cannot_merge_even_after_a_stop() {
    let mut w = ws();
    ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
    start(&mut w, "BAU-150");
    advance(&mut w, 10);
    for actor in [Actor::Agent, Actor::System] {
        assert_eq!(
            w.apply(merge(87, MergeMethod::Squash), actor, T),
            Err(ActionError::MergeNotAllowed(actor))
        );
    }
}
