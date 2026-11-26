//! What is stuck: the entries that block someone, why, and who has to act.

use cockpit_i18n::{Msg, msg};

use super::{in_overview, review_pending};
use crate::agent::AgentRun;
use crate::model::{Item, Person, PrKey, PrRef, TicketStatus};
use crate::workspace::Workspace;
use crate::{Checks, Kind};

/// Why an entry is stuck. The order is also the urgency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StuckKind {
    ChecksFailed,
    /// A limit (budget, time limit) has stopped the agent.
    AgentStopped,
    /// The agent has a follow-up question and waits for the answer.
    QuestionWaiting,
    PlanWaiting,
    AgentWaiting,
    ReviewWaiting,
    NoBranch,
}

/// An entry that blocks someone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stuck {
    pub item_id: String,
    pub key: String,
    pub title: String,
    pub kind: StuckKind,
    /// Who has to do something for things to move on.
    pub who: Person,
    /// The reason as a full sentence.
    pub reason: Msg,
    /// The pull request, if it is about the review.
    pub review: Option<PrKey>,
    /// Ticket whose agent plan waits for approval.
    pub plan: Option<String>,
    /// Ticket where the agent asked a question.
    pub question: Option<String>,
}

/// The first rule that matches the entry. An entry is stuck only once.
fn classify(ws: &Workspace, item: &Item) -> Option<(StuckKind, Person, Msg)> {
    let me = Person::me();
    let pr = item.pr.as_ref();

    if let Some(pr) = pr.filter(|p| p.checks == Checks::Failed) {
        return Some((
            StuckKind::ChecksFailed,
            item.owner.clone(),
            if item.owner == Person::me() {
                msg!(CoreMiscStuckChecksMe, pr = pr.key().to_string())
            } else {
                msg!(
                    CoreMiscStuckChecksOther,
                    pr = pr.key().to_string(),
                    name = item.owner.name_msg()
                )
            },
        ));
    }

    if let Some(reason) = ws
        .runs
        .iter()
        .find(|r| item.matches_run_key(&r.ticket_key))
        .and_then(|r| r.stop_reason())
    {
        return Some((
            StuckKind::AgentStopped,
            me,
            msg!(CoreMiscStuckStopped, reason = reason.label_msg()),
        ));
    }

    if let Some(question) = ws
        .runs
        .iter()
        .find(|r| item.matches_run_key(&r.ticket_key) && r.is_asking())
        .and_then(|r| r.question.as_ref())
    {
        return Some((
            StuckKind::QuestionWaiting,
            me,
            msg!(CoreMiscStuckQuestion, text = &question.text),
        ));
    }

    if ws
        .runs
        .iter()
        .any(|r| item.matches_run_key(&r.ticket_key) && r.is_awaiting())
    {
        return Some((StuckKind::PlanWaiting, me, msg!(CoreMiscStuckPlan)));
    }

    let waiting_run = ws.runs.iter().find(|r| {
        item.matches_run_key(&r.ticket_key)
            && r.is_done()
            && r.pr_key().is_some_and(|n| review_pending(ws, n))
    });
    if let Some(number) = waiting_run.and_then(AgentRun::pr_key) {
        return Some((
            StuckKind::AgentWaiting,
            me,
            msg!(CoreMiscStuckAgentDone, pr = number.to_string()),
        ));
    }

    if let Some(pr) = pr.filter(|p| {
        item.kind == Kind::PullRequest
            && item.owner != me
            && !p.draft
            && p.checks == Checks::Passed
            && review_pending(ws, p.key())
    }) {
        return Some((
            StuckKind::ReviewWaiting,
            me,
            msg!(
                CoreMiscStuckReview,
                name = item.owner.name_msg(),
                pr = pr.key().to_string()
            ),
        ));
    }

    // If the agent is working on it, it creates the branch itself.
    let agent_working = ws
        .runs
        .iter()
        .any(|r| item.matches_run_key(&r.ticket_key) && r.is_active());
    if item.is_ticket()
        && item.status == TicketStatus::InProgress
        && item.branch.is_none()
        && !agent_working
    {
        return Some((
            StuckKind::NoBranch,
            item.owner.clone(),
            if item.owner == Person::me() {
                msg!(CoreMiscStuckNoBranchMe)
            } else {
                msg!(CoreMiscStuckNoBranchOther, name = item.owner.name_msg())
            },
        ));
    }
    None
}

/// Entries that are stuck, most urgent first. Done entries do not count.
pub fn stuck(ws: &Workspace) -> Vec<Stuck> {
    let mut list: Vec<Stuck> = ws
        .items
        .iter()
        .filter(|i| in_overview(ws, i))
        .filter_map(|item| {
            let (kind, who, reason) = classify(ws, item)?;
            // The key of the run that waits (for follow-up jobs not the one of the entry).
            let waiting = |ok: fn(&crate::AgentRun) -> bool| {
                ws.runs
                    .iter()
                    .find(|r| item.matches_run_key(&r.ticket_key) && ok(r))
                    .map(crate::AgentRun::key)
            };
            Some(Stuck {
                item_id: item.id.clone(),
                key: item.key.clone(),
                title: item.title.clone(),
                kind,
                who,
                reason,
                review: item.pr.as_ref().map(PrRef::key),
                plan: waiting(crate::AgentRun::is_awaiting)
                    .filter(|_| kind == StuckKind::PlanWaiting),
                question: waiting(crate::AgentRun::is_asking)
                    .filter(|_| kind == StuckKind::QuestionWaiting),
            })
        })
        .collect();
    // Stable: equally urgent ones stay in inbox order.
    list.sort_by_key(|s| s.kind);
    list
}
