use super::*;

/// Brings BAU-142 up to the follow-up question.
fn asking() -> AgentRun {
    let mut r = run();
    r.start(direct());
    work(&mut r);
    work(&mut r);
    ask(&mut r);
    r
}

#[test]
fn the_agent_asks_a_question_and_waits() {
    let r = asking();
    assert!(r.is_asking() && r.is_active() && !r.is_running());
    assert_eq!(r.completed, 2, "the question is not a step");
    let q = r.question.clone().unwrap();
    assert!(q.text.contains("deprecated"));
    assert_eq!(q.suggestions.len(), 2);
    let steps = r.steps();
    assert_eq!(steps[2].title, "Frage an dich");
    assert_eq!(steps[2].state, StepState::Active);
    assert!(steps[3..].iter().all(|s| s.state == StepState::Pending));
    assert!(
        r.log()
            .contains("› Agent: Remove the old style or mark it as deprecated?"),
        "{}",
        r.log()
    );
}

#[test]
fn an_answer_continues_the_run_and_stays_in_the_history() {
    let mut r = asking();
    assert!(!r.answer("   "), "empty does not count");
    assert!(r.is_asking());
    assert!(r.answer("  Mark it as deprecated "));
    assert!(r.is_running() && r.question.is_none());
    assert!(!r.answer("noch einmal"), "answer only once");
    let last = r.messages.last().unwrap();
    assert_eq!(
        (last.from, last.kind, last.text.as_str()),
        (Speaker::User, MessageKind::Answer, "Mark it as deprecated")
    );
    assert!(r.log().contains("› Du: Mark it as deprecated"));
    work(&mut r);
    finish(&mut r);
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
            .ends_with("Deine Antwort: Mark it as deprecated")
    );
}

#[test]
fn a_hint_is_acknowledged_when_the_container_got_it() {
    let mut r = run();
    r.start(direct());
    assert!(!r.steer("  "), "empty does not count");
    assert!(r.steer(" Auch den Slider prüfen "));
    assert_eq!(r.pending_hints, ["Auch den Slider prüfen"]);
    assert!(
        !r.messages.iter().any(|m| m.kind == MessageKind::Ack),
        "not yet taken into account"
    );
    r.report(
        AgentReport::HintsDelivered { count: 1 },
        &AgentLimits::unlimited(),
        0,
    );
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
            .contains("› Agent: Hinweis erhalten: Auch den Slider prüfen")
    );
}

#[test]
fn hints_only_work_while_the_agent_works() {
    let mut r = run();
    assert!(!r.steer("too early"), "idle");
    r.start(AgentOptions::default());
    assert!(
        !r.steer("Plan noch offen"),
        "that is what the plan note is for"
    );
    r.report(
        AgentReport::Plan {
            summary: "Plan".into(),
            files: vec!["a.rs".into()],
            steps: vec!["Change it".into()],
        },
        &AgentLimits::unlimited(),
        0,
    );
    r.approve();
    assert!(r.steer("jetzt"));
    finish(&mut r);
    assert!(!r.steer("zu spät"), "done");
}

#[test]
fn pause_and_resume() {
    let mut r = run();
    assert!(!r.pause() && !r.resume());
    r.start(direct());
    work(&mut r);
    assert!(r.pause() && r.is_paused() && r.is_active());
    assert!(!r.pause(), "schon pausiert");
    assert_eq!(r.completed, 1);
    assert_eq!(r.steps()[1].state, StepState::Active);
    assert!(r.steps()[1].text.starts_with("Pausiert"));
    assert!(r.log().contains("Lauf pausiert."));
    // Hints can also be sent while paused and wait until resume.
    assert!(r.steer("Während der Pause"));
    assert_eq!(r.pending_hints.len(), 1);
    assert!(r.resume() && r.is_running() && !r.resume());
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
    // A new run starts a fresh conversation, and may ask again.
    r.start(direct());
    assert_eq!(r.messages.len(), 1);
    ask(&mut r);
    assert!(r.is_asking());
}

#[test]
fn the_history_tells_the_story() {
    let mut r = run();
    r.start(AgentOptions {
        hint: "Bitte vorsichtig".into(),
        ..AgentOptions::default()
    });
    r.revise("Auch Tests");
    r.report(
        AgentReport::Plan {
            summary: "Plan".into(),
            files: vec!["a.rs".into()],
            steps: vec!["Change it".into()],
        },
        &AgentLimits::unlimited(),
        0,
    );
    r.approve();
    let texts: Vec<_> = r.messages.iter().map(|m| (m.from, m.kind)).collect();
    assert_eq!(
        texts,
        [
            (Speaker::Agent, MessageKind::Info),
            (Speaker::User, MessageKind::Hint),
            (Speaker::Agent, MessageKind::Info),
            (Speaker::User, MessageKind::Hint),
            (Speaker::Agent, MessageKind::Info),
            (Speaker::User, MessageKind::Info),
        ]
    );
}
