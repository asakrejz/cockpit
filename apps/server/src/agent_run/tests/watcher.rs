use super::*;

#[test]
fn watcher_starts_plan_and_run_each_after_state() {
    let mut ws = demo::workspace();
    assert!(plan_instructions(&ws, &HashMap::new(), &[]).is_empty());
    start(&mut ws, true);
    assert_eq!(
        plan_instructions(&ws, &HashMap::new(), &[]),
        [Instruction::Start("BAU-142".into(), Mode::Plan)]
    );
    // With a running plan container nothing happens until the plan is there.
    assert!(plan_instructions(&ws, &running(Mode::Plan), &[]).is_empty());
    ws.report_agent(
        "BAU-142",
        AgentReport::Plan {
            summary: "s".into(),
            files: vec![],
            steps: vec![],
        },
        T,
    )
    .unwrap();
    assert_eq!(
        plan_instructions(&ws, &running(Mode::Plan), &[]),
        [Instruction::Stop("BAU-142".into())],
        "a finished plan container is no longer needed"
    );
    assert!(
        plan_instructions(&ws, &HashMap::new(), &[]).is_empty(),
        "wartet auf die Freigabe"
    );
    ws.apply(
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plan_instructions(&ws, &HashMap::new(), &[]),
        [Instruction::Start("BAU-142".into(), Mode::Implement)]
    );
    // A run that has just failed does not restart immediately.
    assert!(plan_instructions(&ws, &HashMap::new(), &["BAU-142".to_string()]).is_empty());
}

#[test]
fn watcher_stops_at_cancel_limit_and_orphaned_containern() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    assert!(plan_instructions(&ws, &running(Mode::Implement), &[]).is_empty());
    let mut stop = ws.clone();
    stop.apply(
        Action::CancelAgent {
            ticket: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plan_instructions(&stop, &running(Mode::Implement), &[]),
        [Instruction::Stop("BAU-142".into())]
    );
    // Limit reached: halted.
    let mut limit = ws.clone();
    let mut l = limit.limits.clone();
    l.run_budget_cents = Some(1);
    limit.set_limits(l);
    limit
        .report_agent(
            "BAU-142",
            AgentReport::Step {
                title: "t".into(),
                text: String::new(),
                input_tokens: 1_000_000,
                output_tokens: 0,
                millis: 1,
            },
            T,
        )
        .unwrap();
    assert_eq!(
        plan_instructions(&limit, &running(Mode::Implement), &[]),
        [Instruction::Stop("BAU-142".into())]
    );
    // Container for a run that does not exist.
    let empty = demo::workspace();
    assert_eq!(
        plan_instructions(&empty, &running(Mode::Implement), &[]),
        [Instruction::Stop("BAU-142".into())]
    );
}

#[test]
fn watcher_passes_pause_answer_and_hints_further() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    ws.apply(
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plan_instructions(&ws, &running(Mode::Implement), &[]),
        [Instruction::Pause("BAU-142".into())]
    );
    ws.apply(
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    let mut i = info(Mode::Implement);
    i.paused = true;
    let mut m = HashMap::from([("BAU-142".to_string(), i)]);
    assert_eq!(
        plan_instructions(&ws, &m, &[]),
        [Instruction::Resume("BAU-142".into())]
    );

    // Question and answer.
    ws.report_agent(
        "BAU-142",
        AgentReport::Question {
            text: "Welcher?".into(),
            suggestions: vec![],
        },
        T,
    )
    .unwrap();
    m.get_mut("BAU-142").unwrap().paused = false;
    m.get_mut("BAU-142").unwrap().question_open = true;
    assert!(
        plan_instructions(&ws, &m, &[]).is_empty(),
        "as long as it is unanswered"
    );
    ws.apply(
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "der blaue".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plan_instructions(&ws, &m, &[]),
        [Instruction::Answer("BAU-142".into(), "der blaue".into())]
    );
    m.get_mut("BAU-142").unwrap().question_open = false;

    // Hints go out exactly once.
    ws.apply(
        Action::SteerAgent {
            run: "BAU-142".into(),
            text: "Kleine Commits".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plan_instructions(&ws, &m, &[]),
        [Instruction::Hints(
            "BAU-142".into(),
            vec!["Kleine Commits".into()]
        )]
    );
    m.get_mut("BAU-142").unwrap().hints_sent = 1;
    assert!(plan_instructions(&ws, &m, &[]).is_empty());
}
