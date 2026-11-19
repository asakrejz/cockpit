//! Team view: who works on what and where something is stuck.
//!
//! Everything here is pure functions over the [`Workspace`]. They change nothing and
//! know no UI; the overview page only shows their result.

use cockpit_i18n::{Msg, msg};

use crate::agent::AgentRun;
use crate::diagnosis::Diagnosis;
use crate::model::{Item, Person, PrKey, PrRef, TicketStatus, Tone};
use crate::workspace::Workspace;
use crate::{Checks, Kind};

/// This many open entries count as full load.
pub const CAPACITY: usize = 4;

/// How full a person is. Always readable as text too, never just as a color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoadLevel {
    /// Nothing or little open.
    Free,
    /// Well utilized.
    Busy,
    /// At the limit or above.
    Full,
}

/// A person with their open entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub person: Person,
    /// Tickets in status „Offen“.
    pub open: usize,
    /// Tickets in status „In Arbeit“.
    pub in_progress: usize,
    /// Tickets in status „Review“.
    pub review: usize,
    /// Open pull requests.
    pub pull_requests: usize,
    /// IDs of the entries, in inbox order.
    pub item_ids: Vec<String>,
}

impl Member {
    fn new(person: Person) -> Self {
        Self {
            person,
            open: 0,
            in_progress: 0,
            review: 0,
            pull_requests: 0,
            item_ids: Vec::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.open + self.in_progress + self.review + self.pull_requests
    }

    pub fn load_level(&self) -> LoadLevel {
        match self.total() {
            0..=1 => LoadLevel::Free,
            n if n < CAPACITY => LoadLevel::Busy,
            _ => LoadLevel::Full,
        }
    }

    /// Share of the capacity in percent, capped at 100.
    pub fn load_percent(&self) -> usize {
        (self.total() * 100 / CAPACITY).min(100)
    }
}

/// An entry counts as long as it is not done.
fn is_open(item: &Item) -> bool {
    !item.read && item.status != TicketStatus::Done
}

/// A review is waiting if it exists and has not been sent yet.
fn review_pending(ws: &Workspace, key: impl Into<PrKey>) -> bool {
    ws.review(key).is_some_and(|r| !r.sent)
}

/// All people with their open entries: first „Du“ and the agent, then the
/// others in the order in which they appear in the inbox.
///
/// The agent counts what it is currently implementing (In progress) and what it has finished and
/// is waiting for a review (Review). These tickets still belong to their owner too.
pub fn members(ws: &Workspace) -> Vec<Member> {
    let mut list = vec![Member::new(Person::me()), Member::new(Person::agent())];
    let slot = |list: &mut Vec<Member>, person: &Person| match list
        .iter()
        .position(|m| m.person == *person)
    {
        Some(i) => i,
        None => {
            list.push(Member::new(person.clone()));
            list.len() - 1
        }
    };

    for item in ws.items.iter().filter(|i| is_open(i) && ws.team_visible(i)) {
        let i = slot(&mut list, &item.owner);
        let member = &mut list[i];
        match (item.kind, item.status) {
            (Kind::PullRequest, _) => member.pull_requests += 1,
            (Kind::Ticket, TicketStatus::InProgress) => member.in_progress += 1,
            (Kind::Ticket, TicketStatus::Review) => member.review += 1,
            (Kind::Ticket, _) => member.open += 1,
        }
        member.item_ids.push(item.id.clone());
    }

    let agent = &mut list[1];
    // Per entry the agent counts once: an active run (also a follow-up job) takes precedence over
    // a finished one waiting for review.
    let mut active_ids: Vec<String> = Vec::new();
    for run in &ws.runs {
        let Some(item) = ws
            .item_of_run(&run.ticket_key)
            .filter(|i| is_open(i) && ws.team_visible(i))
        else {
            continue;
        };
        let seen = agent.item_ids.contains(&item.id);
        if run.is_active() {
            if active_ids.contains(&item.id) {
                continue;
            }
            if seen {
                // The entry had been counted as „Review“ and is now back in progress.
                agent.review = agent.review.saturating_sub(1);
            } else {
                agent.item_ids.push(item.id.clone());
            }
            active_ids.push(item.id.clone());
            agent.in_progress += 1;
        } else if !seen && run.is_done() && run.pr_key().is_some_and(|n| review_pending(ws, n)) {
            agent.review += 1;
            agent.item_ids.push(item.id.clone());
        }
    }
    list
}

/// The member the overview shows. `None` for „everybody“: no choice, or a name nobody
/// carries (the person has nothing open any more).
pub fn chosen<'a>(list: &'a [Member], name: Option<&str>) -> Option<&'a Member> {
    name.and_then(|n| list.iter().find(|m| m.person.name == n))
}

/// The stuck entries the overview shows: all of them, or those of the chosen member.
pub fn stuck_of(all: Vec<Stuck>, member: Option<&Member>) -> Vec<Stuck> {
    match member {
        Some(m) => all
            .into_iter()
            .filter(|s| m.item_ids.contains(&s.item_id))
            .collect(),
        None => all,
    }
}

/// The IDs of the entries the overview lists: those of the chosen member, or every open entry
/// once (also when two members hold it, like a ticket and the agent). Inbox order.
pub fn entry_ids(ws: &Workspace, list: &[Member], member: Option<&Member>) -> Vec<String> {
    match member {
        Some(m) => m.item_ids.clone(),
        None => ws
            .items
            .iter()
            .filter(|i| list.iter().any(|m| m.item_ids.contains(&i.id)))
            .map(|i| i.id.clone())
            .collect(),
    }
}

/// Short state of an entry for lists: for tickets the status, for pull requests
/// the state of checks and review.
pub fn state_label(item: &Item) -> (String, Tone) {
    match (item.kind, &item.pr) {
        (Kind::PullRequest, Some(pr)) => pr.status(),
        (Kind::PullRequest, None) => ("Pull Request".into(), Tone::Gray),
        (Kind::Ticket, _) => {
            let label = TicketStatus::OPTIONS
                .iter()
                .find(|(id, _)| *id == item.status.id())
                .map_or("", |(_, label)| *label);
            let tone = match item.status {
                TicketStatus::Open => Tone::Gray,
                TicketStatus::InProgress => Tone::Blue,
                TicketStatus::Review => Tone::Orange,
                TicketStatus::Done => Tone::Green,
            };
            (label.into(), tone)
        }
    }
}

/// Why an entry is stuck. The order is also the urgency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StuckKind {
    ChecksFailed,
    /// A limit (budget, time limit) has stopped the agent.
    AgentStopped,
    /// The agent has a follow-up question and waits for the answer.
    QuestionWaiting,
    /// The agent has examined a red check and waits whether it should adopt the fix.
    DiagnosisWaiting,
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
    /// Run whose diagnosis waits for your decision.
    pub diagnosis: Option<String>,
}

/// The first rule that matches the entry. An entry is stuck only once.
fn classify(ws: &Workspace, item: &Item) -> Option<(StuckKind, Person, Msg)> {
    let me = Person::me();
    let pr = item.pr.as_ref();

    // The diagnosis comes first: the check is red, but the agent already has a proposal.
    if let Some(run) = ws
        .runs
        .iter()
        .find(|r| item.matches_run_key(&r.ticket_key) && r.is_diagnosed())
    {
        let cause = run
            .diagnosis
            .as_ref()
            .map_or_else(|| msg!(CoreDiagnosisCauseDefault), Diagnosis::cause_message);
        return Some((
            StuckKind::DiagnosisWaiting,
            me,
            msg!(CoreMiscStuckDiagnosis, cause = cause),
        ));
    }

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
                    name = &item.owner.name
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
            msg!(CoreMiscStuckStopped, reason = reason.label()),
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
                name = &item.owner.name,
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
                msg!(CoreMiscStuckNoBranchOther, name = &item.owner.name)
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
        .filter(|i| is_open(i) && ws.team_visible(i))
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
                diagnosis: waiting(crate::AgentRun::is_diagnosed)
                    .filter(|_| kind == StuckKind::DiagnosisWaiting),
            })
        })
        .collect();
    // Stable: equally urgent ones stay in inbox order.
    list.sort_by_key(|s| s.kind);
    list
}

#[cfg(test)]
mod tests;
