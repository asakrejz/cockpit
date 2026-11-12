use super::*;

#[test]
fn only_the_agent_may_report() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    for actor in [Actor::User, Actor::System] {
        let before = w.clone();
        let a = Action::ReportAgent {
            run: "BAU-142".into(),
            report: AgentReport::HintsDelivered { count: 1 },
        };
        assert_eq!(w.apply(a, actor, T), Err(ActionError::AgentOnly));
        assert_eq!(w, before);
    }
}

#[test]
fn a_real_run_logs_as_agent_and_books_the_cost() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Started {
            detail: "docker, Modus umsetzen".into(),
        },
    )
    .unwrap();
    let applied = report(
        &mut w,
        "BAU-142",
        AgentReport::Step {
            title: "Datei lesen".into(),
            text: "src/ring.rs".into(),
            input_tokens: 10_000,
            output_tokens: 1_000,
            millis: 4_000,
        },
    )
    .unwrap();
    assert_eq!(applied.logged, 1);
    let last = w.log.last().unwrap();
    assert_eq!(last.actor, Actor::Agent);
    assert_eq!(last.target, Some(Target::Run("BAU-142".into())));
    assert_eq!(w.spent_month(T).input_tokens, 10_000);
    // The simulated tick does not touch the real run.
    assert_eq!(w.running_agents(), 0);
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    // Done: branch on the ticket, no invented pull request.
    let reviews = w.reviews.len();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Done {
            summary: "Fokus-Ring ergänzt".into(),
        },
    )
    .unwrap();
    assert_eq!(w.reviews.len(), reviews);
    let item = w.item_by_key("BAU-142").unwrap();
    assert_eq!(
        item.branch.as_deref(),
        Some("claude/BAU-142-toggle-fokus-ring-fehlt")
    );
    assert!(item.agent_note.as_deref().unwrap().contains("gepusht"));
}

#[test]
fn a_failed_real_run_ends_with_the_reason_and_clears_the_note() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Failed {
            reason: "Zeitlimit des Containers überschritten".into(),
        },
    )
    .unwrap();
    let run = w.run("BAU-142").unwrap();
    assert!(run.is_idle());
    assert_eq!(
        run.ended.as_deref(),
        Some("Zeitlimit des Containers überschritten")
    );
    assert_eq!(w.item_by_key("BAU-142").unwrap().agent_note, None);
    // Nothing active any more: further reports are rejected and change nothing.
    let before = w.clone();
    assert_eq!(
        report(&mut w, "BAU-142", AgentReport::HintsDelivered { count: 1 }),
        Err(ActionError::NothingToDo)
    );
    assert_eq!(w, before);
}

#[test]
fn the_budget_in_the_workspace_stops_a_real_run() {
    let mut w = ws();
    let mut limits = w.limits.clone();
    limits.run_budget_cents = Some(50);
    assert!(w.set_limits(limits));
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Step {
            title: "Viel gelesen".into(),
            text: String::new(),
            input_tokens: 1_000_000,
            output_tokens: 0,
            millis: 1,
        },
    )
    .unwrap();
    assert!(w.run("BAU-142").unwrap().is_stopped());
    assert_eq!(w.stopped_agents(), 1);
}

#[test]
fn a_real_run_survives_save_and_load() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        },
        2,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Plan {
            summary: "Plan".into(),
            files: vec!["a.rs".into()],
            steps: vec!["x".into()],
        },
    )
    .unwrap();
    let (back, _) = persist::restore(Some(&persist::save(&w)));
    assert_eq!(back, w);
    assert!(back.run("BAU-142").unwrap().agent_plan.is_some());
    let json = serde_json::to_string(&Action::ReportAgent {
        run: "X".into(),
        report: AgentReport::HintsDelivered { count: 1 },
    })
    .unwrap();
    assert!(serde_json::from_str::<Action>(&json).is_ok());
}
