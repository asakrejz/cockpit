//! Helpers of the tests: what the container of an agent would report, applied to a workspace.

use crate::action::{Action, Actor, Applied};
use crate::agent::{AgentReport, STEPS};
use crate::persist::{self, LoadError};
use crate::workspace::Workspace;

/// The consumption of each step (input tokens, output tokens, milliseconds). The same for every
/// run and step, so that the limits of the tests reach the same point every time.
const USAGE: [(u64, u64, u64); STEPS] = [
    (8_200, 900, 18_000),
    (15_600, 1_400, 22_000),
    (46_300, 6_800, 85_000),
    (28_900, 2_300, 61_000),
    (9_400, 1_500, 27_000),
];

/// One step of every running run, reported at `at_ms`: the consumption of the step, and after the
/// last step the result (unless a limit has stopped the run).
pub(crate) fn advance_at(w: &mut Workspace, at_ms: u64) {
    let running: Vec<(String, usize)> = w
        .runs
        .iter()
        .filter(|r| r.is_running())
        .map(|r| (r.key(), r.completed))
        .collect();
    for (key, step) in running {
        let (input_tokens, output_tokens, millis) = USAGE[step.min(STEPS - 1)];
        let _ = w.apply(
            Action::ReportAgent {
                run: key.clone(),
                report: AgentReport::Step {
                    title: format!("Step {}", step + 1),
                    text: String::new(),
                    input_tokens,
                    output_tokens,
                    millis,
                },
            },
            Actor::Agent,
            at_ms,
        );
        if step + 1 >= STEPS && w.run(&key).is_some_and(|r| r.is_running()) {
            let _ = w.apply(
                Action::ReportAgent {
                    run: key,
                    report: AgentReport::Done {
                        summary: "Done".into(),
                    },
                },
                Actor::Agent,
                at_ms,
            );
        }
    }
}

/// [`advance_at`] without a clock.
pub(crate) fn advance(w: &mut Workspace) {
    advance_at(w, 0);
}

/// The container of run `key` asks a question.
pub(crate) fn question(w: &mut Workspace, key: &str, text: &str) {
    w.apply(
        Action::ReportAgent {
            run: key.into(),
            report: AgentReport::Question {
                text: text.into(),
                suggestions: Vec::new(),
            },
        },
        Actor::Agent,
        0,
    )
    .unwrap();
}

/// The workspace as the server stores it (revision 0).
pub(crate) fn saved(w: &Workspace) -> String {
    persist::save_with_revision(w, 0)
}

/// A stored text read back, without its revision.
pub(crate) fn loaded(text: &str) -> Result<Workspace, LoadError> {
    persist::load_with_revision(text).map(|(w, _)| w)
}

/// The text of the first log entry that an applied action added (empty if it added none).
pub(crate) fn first_text<'a>(w: &'a Workspace, applied: &Applied) -> &'a str {
    w.log
        .get(w.log.len() - applied.logged)
        .map_or("", |entry| entry.text.as_str())
}
