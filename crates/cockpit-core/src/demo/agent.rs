//! The sample data of the agent: token counts per step and the question.

use super::*;

/// Consumption of a demo step: input tokens, output tokens and runtime in milliseconds.
/// Fixed and dependent on the ticket, so that demo and tests always show the same. A real agent
/// reports these values itself (see [`AgentRun::record_usage`]).
pub fn step_tokens(ticket_key: &str, step: usize) -> (u64, u64, u64) {
    const INPUT: [u64; STEPS] = [8_200, 15_600, 46_300, 28_900, 9_400];
    const OUTPUT: [u64; STEPS] = [900, 1_400, 6_800, 2_300, 1_500];
    const MILLIS: [u64; STEPS] = [18_000, 22_000, 85_000, 61_000, 27_000];
    let seed = ticket_key
        .bytes()
        .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b)))
        % 7;
    let i = step.min(STEPS - 1);
    (
        INPUT[i] + seed * 1_100,
        OUTPUT[i] + seed * 140,
        MILLIS[i] + seed * 2_500,
    )
}

/// The question the demo agent asks for a ticket: before which step (number
/// of completed steps) and what. For all other tickets it asks nothing.
pub fn agent_question(ticket_key: &str) -> Option<(usize, Question)> {
    match ticket_key {
        "BAU-142" => Some((
            2,
            Question {
                text: "Soll der alte Stil entfernt oder als deprecated markiert werden?".into(),
                suggestions: vec![
                    "Entfernen".into(),
                    "Als deprecated markieren".into(),
                    "Erst markieren, später entfernen".into(),
                ],
            },
        )),
        // Follow-up job „Kommentare in #91 umsetzen“.
        "review-asakrejz~rust-playground-91" => Some((
            2,
            Question {
                text: "Soll ich vor dem Push auch die Browser-Tests laufen lassen? Das dauert etwa zwei Minuten."
                    .into(),
                suggestions: vec![
                    "Ja, mit Browser-Tests".into(),
                    "Nein, Lint und Unit-Tests genügen".into(),
                ],
            },
        )),
        _ => None,
    }
}
