use super::*;

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
