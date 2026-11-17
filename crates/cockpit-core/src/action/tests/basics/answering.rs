use super::*;

#[test]
fn answering_continues_the_agent_and_is_logged() {
    let mut w = asking_ws();
    assert_eq!(
        w.item("bau142").unwrap().agent_note.as_deref(),
        Some("Der Agent hat eine Frage an dich")
    );
    let applied = ok(
        &mut w,
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: " Als deprecated markieren ".into(),
        },
        1,
    );
    assert_eq!(
        applied.summary,
        "Frage des Agenten für BAU-142 beantwortet: „Als deprecated markieren“"
    );
    let last = w.log.last().unwrap();
    assert_eq!(
        (last.actor, &last.target),
        (Actor::User, &Some(Target::Run("BAU-142".into())))
    );
    assert!(w.run("BAU-142").unwrap().is_running());
    assert_eq!(
        w.item("bau142").unwrap().agent_note.as_deref(),
        Some("Der Agent arbeitet an der Umsetzung")
    );
}

#[test]
fn invalid_answers_change_nothing() {
    let mut w = asking_ws();
    fails(
        &mut w,
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "  ".into(),
        },
        ActionError::InvalidInput,
    );
    let mut idle = ws();
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
        fails(&mut idle, action, ActionError::NothingToDo);
    }
    // Whoever asks is not running: pausing is possible only after the answer.
    fails(
        &mut w,
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
        ActionError::NothingToDo,
    );
}

#[test]
fn steering_is_acknowledged_by_the_agent_in_the_next_step() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: direct(),
        },
        1,
    );
    fails(
        &mut w,
        Action::SteerAgent {
            run: "BAU-150".into(),
            text: "".into(),
        },
        ActionError::InvalidInput,
    );
    ok(
        &mut w,
        Action::SteerAgent {
            run: "BAU-150".into(),
            text: "Bitte auch die Doku".into(),
        },
        1,
    );
    // The tick logs the acknowledged hint and the step.
    let tick = w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
    assert_eq!(tick.logged, 2);
    assert_eq!(tick.summary, "BAU-150: 1 Hinweis berücksichtigt");
    let run = w.run("BAU-150").unwrap();
    assert!(
        run.messages
            .iter()
            .any(|m| m.text == "Hinweis erhalten: Bitte auch die Doku")
    );
}
