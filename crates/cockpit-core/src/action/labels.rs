//! The small words the log sentences and refusals are built from, as messages: statuses,
//! priorities, reasons and so on. They are nested into the sentences.
//! (A text from outside goes through `Msg::raw`, which is the catalog key `CommonRaw`.)

use cockpit_i18n::{ArgValue, Msg, msg};

use super::*;
use crate::agent::{FollowupTask, RunKind};

pub(crate) fn on_off(on: bool) -> Msg {
    if on {
        msg!(CoreActionOn)
    } else {
        msg!(CoreActionOff)
    }
}

pub(crate) fn comments(n: usize) -> Msg {
    msg!(CoreActionComments, n = n)
}

pub(crate) fn who(actor: Actor) -> Msg {
    match actor {
        Actor::Agent => msg!(CoreActionWhoAgent),
        Actor::System => msg!(CoreActionWhoSystem),
        Actor::User => msg!(CoreActionWhoUser),
    }
}

pub(crate) fn status(status: TicketStatus) -> Msg {
    status.name_msg()
}

pub(crate) fn priority(priority: Priority) -> Msg {
    match priority {
        Priority::Highest => msg!(CoreActionPriorityHighest),
        Priority::High => msg!(CoreActionPriorityHigh),
        Priority::Medium => msg!(CoreActionPriorityMedium),
        Priority::Low => msg!(CoreActionPriorityLow),
        Priority::Lowest => msg!(CoreActionPriorityLowest),
    }
}

pub(crate) fn rule(rule: Rule) -> Msg {
    match rule {
        Rule::Mention => msg!(CoreActionRuleMention),
        Rule::ReviewRequested => msg!(CoreActionRuleReview),
        Rule::Blocked => msg!(CoreActionRuleBlocked),
        Rule::Overdue => msg!(CoreActionRuleOverdue),
        Rule::AgentFinished => msg!(CoreActionRuleAgent),
        Rule::Assigned => msg!(CoreActionRuleAssigned),
    }
}

pub(crate) fn verdict(verdict: Verdict) -> Msg {
    match verdict {
        Verdict::Comment => msg!(CoreActionVerdictComment),
        Verdict::Approve => msg!(CoreActionVerdictApprove),
        Verdict::RequestChanges => msg!(CoreActionVerdictChanges),
    }
}

pub(crate) fn method(method: MergeMethod) -> Msg {
    match method {
        MergeMethod::Merge => msg!(CoreActionMethodMerge),
        MergeMethod::Squash => msg!(CoreActionMethodSquash),
        MergeMethod::Rebase => msg!(CoreActionMethodRebase),
    }
}

pub(crate) fn reason(reason: NotifyReason) -> Msg {
    match reason {
        NotifyReason::ReviewRequested => msg!(CoreActionReasonReview),
        NotifyReason::Mention => msg!(CoreActionReasonMention),
        NotifyReason::AgentWaiting => msg!(CoreActionReasonWaiting),
        NotifyReason::AgentFinished => msg!(CoreActionReasonFinished),
        NotifyReason::ChecksFailed => msg!(CoreActionReasonChecks),
        NotifyReason::Assigned => msg!(CoreActionReasonAssigned),
        NotifyReason::AgentStopped => msg!(CoreActionReasonStopped),
    }
}

pub(crate) fn channel(channel: Channel) -> Msg {
    match channel {
        Channel::Browser => msg!(CoreActionChannelBrowser),
    }
}

pub(crate) fn closure(closure: Closure) -> Msg {
    match closure {
        Closure::Merged => msg!(CoreActionClosureMerged),
        Closure::Closed => msg!(CoreActionClosureClosed),
    }
}

/// Which limit halted the run.
pub(crate) fn stop(reason: StopReason) -> Msg {
    match reason {
        StopReason::RunBudget => msg!(CoreActionStopRunBudget),
        StopReason::MonthBudget => msg!(CoreActionStopMonthBudget),
        StopReason::Time => msg!(CoreActionStopTime),
    }
}

/// What is raised when the run resumes.
pub(crate) fn raises(reason: StopReason) -> Msg {
    match reason {
        StopReason::RunBudget => msg!(CoreActionRaisesRunBudget),
        StopReason::MonthBudget => msg!(CoreActionRaisesMonthBudget),
        StopReason::Time => msg!(CoreActionRaisesTime),
    }
}

/// The size of a raise: euros for budgets, minutes for the time limit.
pub(crate) fn raise_amount(reason: StopReason, extra: u64) -> ArgValue {
    match reason {
        StopReason::Time => msg!(CoreActionMinutes, n = extra).into(),
        _ => ArgValue::from(format_euro(extra)),
    }
}

/// The short sentence on the limits for the log.
pub(crate) fn limits_summary(limits: &AgentLimits) -> Msg {
    let money = |v: Option<u64>| -> ArgValue {
        v.map_or_else(
            || msg!(CoreActionUnlimited).into(),
            |c| format_euro(c).into(),
        )
    };
    msg!(
        CoreActionLimitsSummary,
        run = money(limits.run_budget_cents),
        month = money(limits.month_budget_cents),
        time = limits.run_minutes.map_or_else(
            || ArgValue::from(msg!(CoreActionUnlimited)),
            |m| msg!(CoreActionMinutes, n = m).into()
        ),
        parallel = limits.max_parallel
    )
}

/// The rules of a repo as lines for the log.
pub(crate) fn rule_lines(rules: &RepoRules) -> Vec<Msg> {
    let mut lines = Vec::new();
    if !rules.allowed_paths.is_empty() {
        lines.push(msg!(
            CoreActionRulesAllowed,
            paths = rules.allowed_paths.join(", ")
        ));
    }
    if !rules.forbidden_paths.is_empty() {
        lines.push(msg!(
            CoreActionRulesForbidden,
            paths = rules.forbidden_paths.join(", ")
        ));
    }
    if !rules.required_tests.is_empty() {
        lines.push(msg!(
            CoreActionRulesTests,
            tests = rules.required_tests.join(", ")
        ));
    }
    if rules.plan_required {
        lines.push(msg!(CoreActionRulesPlan));
    }
    if !rules.instructions.is_empty() {
        lines.push(msg!(
            CoreActionRulesInstructions,
            text = rules.instructions.clone()
        ));
    }
    lines
}

pub(crate) fn violation(v: &PathViolation) -> Msg {
    match v.kind {
        ViolationKind::Forbidden => {
            msg!(
                CoreActionViolationForbidden,
                file = &v.file,
                pattern = &v.pattern
            )
        }
        ViolationKind::NotAllowed => msg!(CoreActionViolationNotAllowed, file = &v.file),
    }
}

pub(crate) fn blocker(b: MergeBlocker) -> Msg {
    match b {
        MergeBlocker::AlreadyMerged => msg!(CoreActionBlockerMerged),
        MergeBlocker::Draft => msg!(CoreActionBlockerDraft),
        MergeBlocker::ChecksFailed => msg!(CoreActionBlockerChecksFailed),
        MergeBlocker::ChecksRunning => msg!(CoreActionBlockerChecksRunning),
        MergeBlocker::ReviewNotSent => msg!(CoreActionBlockerNotSent),
        MergeBlocker::ChangesRequested => msg!(CoreActionBlockerChanges),
    }
}

/// Short name of a run for log sentences (what `AgentRun::label_msg` says).
pub(crate) fn run_label(run: &AgentRun) -> Msg {
    match &run.kind {
        RunKind::Implement => Msg::raw(run.ticket_key.clone()),
        RunKind::Followup { review, task } => match task {
            FollowupTask::ReviewThreads => {
                msg!(CoreActionRunThreads, review = review.to_string())
            }
            FollowupTask::FailedChecks { .. } => {
                msg!(CoreActionRunFailedChecks, review = review.to_string())
            }
        },
        RunKind::ReviewPr { review } => msg!(CoreActionRunReview, review = review.to_string()),
    }
}
