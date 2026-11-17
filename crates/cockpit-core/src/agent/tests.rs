use super::*;

mod conversation;
mod real;
mod simulation;

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
