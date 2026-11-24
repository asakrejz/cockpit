use super::*;

mod conversation;
mod real;

/// Without plan approval: the run starts immediately.
fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..Default::default()
    }
}

/// One work step as the container reports it.
fn work(r: &mut AgentRun) -> Option<ReportEffect> {
    r.report(
        AgentReport::Step {
            title: "Step".into(),
            text: String::new(),
            input_tokens: 1_000,
            output_tokens: 100,
            millis: 1_000,
        },
        &AgentLimits::unlimited(),
        0,
    )
}

/// The container asks a question.
fn ask(r: &mut AgentRun) {
    r.report(
        AgentReport::Question {
            text: "Remove the old style or mark it as deprecated?".into(),
            suggestions: vec!["Remove it".into(), "Mark it as deprecated".into()],
        },
        &AgentLimits::unlimited(),
        0,
    )
    .unwrap();
}

/// The container is done.
fn finish(r: &mut AgentRun) {
    r.report(
        AgentReport::Done {
            summary: "Done".into(),
        },
        &AgentLimits::unlimited(),
        0,
    )
    .unwrap();
}

fn run() -> AgentRun {
    let mut r = AgentRun::new("BAU-142", "Toggle: Fokus-Ring fehlt im Dark Mode");
    r.comments = 1;
    r.files = vec!["components.css".into(), "tokens.css".into()];
    r
}
