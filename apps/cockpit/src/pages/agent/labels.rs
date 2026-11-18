//! Texts for things the core names: the label of a run, the reason a run was halted and the
//! usage line. The core builds them as messages; they are rendered in the current language here.

use cockpit_core::AgentRun;
use cockpit_core::usage::{StopReason, Usage};
use feature_ui::lang;

/// Short name of a run: the ticket key, for follow-up jobs a short text.
pub(super) fn run_label(run: &AgentRun) -> String {
    run.label_msg().render(lang())
}

/// Which limit halted the run.
pub(super) fn stop_label(reason: StopReason) -> String {
    reason.label_msg().render(lang())
}

/// What is raised when the run resumes.
pub(super) fn raises_label(reason: StopReason) -> String {
    reason.raises_msg().render(lang())
}

/// The size of an increase: euros for budgets, minutes for the time limit.
pub(super) fn raise_amount(reason: StopReason, extra: u64) -> String {
    reason.raise_label_msg(extra).render(lang())
}

/// Tokens, cost and time of a run in one line.
pub(super) fn usage_line(usage: &Usage) -> String {
    usage.line_msg().render(lang())
}
