//! Texts for the enums of `cockpit_core` (their `label()` is German only). Everything the
//! inbox shows for them goes through here, so it follows the language.

use cockpit_core::{Checks, Filter, PrRef, Priority, Rule, SnoozeChoice, TicketStatus};
use feature_ui::*;

pub fn ticket_status(status: TicketStatus) -> String {
    match status {
        TicketStatus::Open => t!(InboxDetailStatusOpen),
        TicketStatus::InProgress => t!(InboxDetailStatusProgress),
        TicketStatus::Review => t!(InboxDetailStatusReview),
        TicketStatus::Done => t!(InboxDetailStatusDone),
    }
}

pub fn priority(priority: Priority) -> String {
    match priority {
        Priority::Highest => t!(InboxDetailPriorityHighest),
        Priority::High => t!(InboxDetailPriorityHigh),
        Priority::Medium => t!(InboxDetailPriorityMedium),
        Priority::Low => t!(InboxDetailPriorityLow),
        Priority::Lowest => t!(InboxDetailPriorityLowest),
    }
}

pub fn snooze_choice(choice: SnoozeChoice) -> String {
    match choice {
        SnoozeChoice::Hour => t!(InboxDetailSnoozeHour),
        SnoozeChoice::Tomorrow => t!(InboxDetailSnoozeTomorrow),
        SnoozeChoice::NextWeek => t!(InboxDetailSnoozeWeek),
        SnoozeChoice::UntilChange => t!(InboxDetailSnoozeChange),
    }
}

pub fn filter_tab(filter: Filter) -> String {
    match filter {
        Filter::All => t!(InboxTabAll),
        Filter::Mine => t!(InboxTabMine),
        Filter::Team => t!(InboxTabTeam),
        Filter::Snoozed => t!(InboxTabSnoozed),
    }
}

pub fn rule(rule: Rule) -> String {
    match rule {
        Rule::Mention => t!(InboxFiltersRuleMention),
        Rule::ReviewRequested => t!(InboxFiltersRuleReview),
        Rule::Blocked => t!(InboxFiltersRuleBlocked),
        Rule::Overdue => t!(InboxFiltersRuleOverdue),
        Rule::AgentFinished => t!(InboxFiltersRuleAgent),
        Rule::Assigned => t!(InboxFiltersRuleAssigned),
    }
}

pub fn rule_hint(rule: Rule) -> String {
    match rule {
        Rule::Mention => t!(InboxFiltersRuleMentionHint),
        Rule::ReviewRequested => t!(InboxFiltersRuleReviewHint),
        Rule::Blocked => t!(InboxFiltersRuleBlockedHint),
        Rule::Overdue => t!(InboxFiltersRuleOverdueHint),
        Rule::AgentFinished => t!(InboxFiltersRuleAgentHint),
        Rule::Assigned => t!(InboxFiltersRuleAssignedHint),
    }
}

/// "#87 · open"
pub fn pr_label(pr: &PrRef) -> String {
    let state = match (pr.merged, pr.draft) {
        (true, _) => t!(InboxDetailPrMerged),
        (false, true) => t!(InboxDetailPrDraft),
        (false, false) => t!(InboxDetailPrOpen),
    };
    t!(InboxDetailPrLabel, number = pr.number, state = state)
}

/// The status line of a pull request (the tone comes from `PrRef::status`).
pub fn pr_status(pr: &PrRef) -> String {
    if pr.merged {
        return t!(InboxDetailPrStatusMerged);
    }
    match (pr.checks, pr.draft) {
        (Checks::Passed, false) => t!(InboxDetailPrStatusReviewOpen),
        (Checks::Passed, true) => t!(InboxDetailPrStatusDraft),
        (Checks::Failed, _) => t!(InboxDetailChecksFailed),
        (Checks::Running, _) => t!(InboxDetailChecksRunning),
        (Checks::Unknown, _) => t!(InboxDetailChecksUnknown),
    }
}

/// The options of a picker with the ids of `ids` and translated labels.
pub fn options<T: Copy>(
    ids: &'static [(&'static str, &'static str)],
    from_id: impl Fn(&str) -> Option<T>,
    label: impl Fn(T) -> String,
) -> Vec<(&'static str, String)> {
    ids.iter()
        .filter_map(|(id, _)| from_id(id).map(|v| (*id, label(v))))
        .collect()
}
