//! Texts, icons and tones for the entries of the overview.

use cockpit_core::{Item, Kind, LoadLevel, Member, StuckKind, Tone, team::Group};
use feature_ui::*;

use crate::pages::inbox::labels;

/// Line "1 open · 2 in progress" – only what exists.
pub(super) fn counts(m: &Member) -> String {
    let parts: Vec<String> = [
        (m.open > 0).then(|| t!(OverviewCountOpen, n = m.open)),
        (m.in_progress > 0).then(|| t!(OverviewCountProgress, n = m.in_progress)),
        (m.review > 0).then(|| t!(OverviewCountReview, n = m.review)),
        (m.pull_requests > 0).then(|| t!(OverviewCountPrs, n = m.pull_requests)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        t!(OverviewNothingOpen)
    } else {
        parts.join(" · ")
    }
}

pub(super) fn kind_label(kind: StuckKind) -> String {
    match kind {
        StuckKind::ChecksFailed => t!(OverviewKindChecks),
        StuckKind::AgentStopped => t!(OverviewKindStopped),
        StuckKind::QuestionWaiting => t!(OverviewKindQuestion),
        StuckKind::PlanWaiting => t!(OverviewKindPlan),
        StuckKind::AgentWaiting => t!(OverviewKindAgent),
        StuckKind::ReviewWaiting => t!(OverviewKindReview),
        StuckKind::NoBranch => t!(OverviewKindNoBranch),
    }
}

pub(super) fn load_label(level: LoadLevel) -> String {
    match level {
        LoadLevel::Free => t!(OverviewLoadFree),
        LoadLevel::Busy => t!(OverviewLoadBusy),
        LoadLevel::Full => t!(OverviewLoadFull),
    }
}

/// What the entry is: the status of a ticket, or the state of a pull request.
pub(super) fn state_text(item: &Item) -> String {
    match (item.kind, &item.pr) {
        (Kind::PullRequest, Some(pr)) => labels::pr_status(pr),
        (Kind::PullRequest, None) => t!(OverviewPullRequest),
        (Kind::Ticket, _) => labels::ticket_status(item.status),
    }
}

pub(super) fn kind_icon(kind: StuckKind) -> &'static str {
    match kind {
        StuckKind::ChecksFailed => icons::ALERT,
        StuckKind::AgentStopped => icons::ALERT,
        StuckKind::QuestionWaiting => icons::SPARKLES,
        StuckKind::PlanWaiting => icons::TASKS,
        StuckKind::AgentWaiting => icons::SPARKLE,
        StuckKind::ReviewWaiting => icons::EYE,
        StuckKind::NoBranch => icons::GIT_BRANCH,
    }
}

pub(super) fn kind_tone(kind: StuckKind) -> Tone {
    match kind {
        StuckKind::ChecksFailed => Tone::Red,
        StuckKind::AgentStopped => Tone::Red,
        StuckKind::QuestionWaiting => Tone::Orange,
        StuckKind::PlanWaiting => Tone::Orange,
        StuckKind::AgentWaiting => Tone::Green,
        StuckKind::ReviewWaiting => Tone::Orange,
        StuckKind::NoBranch => Tone::Indigo,
    }
}

pub(super) fn group_label(group: Group) -> String {
    match group {
        Group::Both => t!(OverviewGroupBoth),
        Group::Jira => t!(OverviewGroupJira),
        Group::GitHub => t!(OverviewGroupGithub),
    }
}
