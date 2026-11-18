//! Texts for things the core names in German: the label of a run, the reason a run was halted and
//! the usage line. The UI looks them up here so that they follow the language.

use cockpit_core::usage::{StopReason, Usage, format_duration, format_euro, format_number};
use cockpit_core::{AgentRun, FollowupTask, RunKind};
use feature_ui::*;

/// Short name of a run: the ticket key, for follow-up jobs plain text.
pub(super) fn run_label(run: &AgentRun) -> String {
    match &run.kind {
        RunKind::Implement => run.ticket_key.clone(),
        RunKind::AddressReview { review, .. } => {
            t!(AgentLabelsRunAddressReview, review = review)
        }
        RunKind::Diagnose { review, check } => {
            t!(AgentLabelsRunDiagnose, check = check, review = review)
        }
        RunKind::Followup { review, task } => match task {
            FollowupTask::ReviewThreads => t!(AgentLabelsRunThreads, review = review),
            FollowupTask::FailedChecks { .. } => {
                t!(AgentLabelsRunFailedChecks, review = review)
            }
        },
        RunKind::ReviewPr { review } => t!(AgentLabelsRunReview, review = review),
    }
}

/// Which limit halted the run.
pub(super) fn stop_label(reason: StopReason) -> String {
    match reason {
        StopReason::RunBudget => t!(AgentLabelsStopRunBudget),
        StopReason::MonthBudget => t!(AgentLabelsStopMonthBudget),
        StopReason::Time => t!(AgentLabelsStopTime),
    }
}

/// What is raised when the run resumes.
pub(super) fn raises_label(reason: StopReason) -> String {
    match reason {
        StopReason::RunBudget => t!(AgentLabelsRaisesRunBudget),
        StopReason::MonthBudget => t!(AgentLabelsRaisesMonthBudget),
        StopReason::Time => t!(AgentLabelsRaisesTime),
    }
}

/// The size of an increase: euros for budgets, minutes for the time limit.
pub(super) fn raise_amount(reason: StopReason, extra: u64) -> String {
    match reason {
        StopReason::Time => t!(AgentLabelsMinutes, n = extra),
        _ => format_euro(extra),
    }
}

/// Tokens, cost and time of a run in one line.
pub(super) fn usage_line(usage: &Usage) -> String {
    t!(
        AgentLabelsUsageLine,
        input = format_number(usage.input_tokens),
        output = format_number(usage.output_tokens),
        cost = format_euro(usage.cost_cents),
        time = format_duration(usage.millis)
    )
}
