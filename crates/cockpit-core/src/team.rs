//! Team view: who works on what and where something is stuck.
//!
//! Everything here is pure functions over the [`Workspace`]. They change nothing and
//! know no UI; the overview page only shows their result.

use cockpit_i18n::{Msg, msg};

use crate::agent::AgentRun;
use crate::model::{Item, Person, PrKey, PrRef, Source, TicketStatus, Tone};
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

/// Where an entry comes from, for the overview: Jira, or GitHub (pull requests and GitHub
/// Projects tickets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Jira,
    GitHub,
}

impl Side {
    pub fn of(source: Source) -> Self {
        match source {
            Source::Jira => Side::Jira,
            Source::GitHub | Source::GitHubProject => Side::GitHub,
        }
    }
}

/// The section of the list a member belongs to when the overview shows „Alle“.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Jira and GitHub: you, the agent and linked people.
    Both,
    Jira,
    GitHub,
}

/// A person with their open entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Identifies the member in the address and the list: the same name in Jira and in GitHub
    /// gives two members with two keys. „Du“ and „Agent“ keep their plain name.
    pub key: String,
    pub person: Person,
    /// The GitHub login linked to this Jira person (see [`crate::people_link`]).
    pub github: Option<String>,
    /// Where the member's open entries come from.
    pub sides: Vec<Side>,
    /// Tickets in status "Open".
    pub open: usize,
    /// Tickets in status "In progress".
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
        // The key is the address of the member's page; you keep one that does not follow the
        // language.
        let key = if person.me {
            crate::model::ME_MARK.to_string()
        } else {
            person.name.clone()
        };
        Self {
            key,
            person,
            github: None,
            sides: Vec::new(),
            open: 0,
            in_progress: 0,
            review: 0,
            pull_requests: 0,
            item_ids: Vec::new(),
        }
    }

    fn note_side(&mut self, side: Side) {
        if !self.sides.contains(&side) {
            self.sides.push(side);
        }
    }

    /// The section of this member in „Alle“. You, the agent and people with entries from both
    /// sources (linked ones) go together; the others to their source.
    pub fn group(&self) -> Group {
        if self.person == Person::me() || self.person.is_agent() {
            return Group::Both;
        }
        match self.sides.as_slice() {
            [Side::Jira] => Group::Jira,
            [Side::GitHub] => Group::GitHub,
            _ => Group::Both,
        }
    }

    /// The source of a member that is not linked yet (a plain Jira person or GitHub user), who
    /// can be linked to a person of the other source. `None` for you, the agent and linked ones.
    pub fn unlinked_side(&self) -> Option<Side> {
        if self.person.nobody {
            None
        } else if self.key.starts_with(JIRA_KEY) {
            Some(Side::Jira)
        } else if self.key.starts_with(GITHUB_KEY) {
            Some(Side::GitHub)
        } else {
            None
        }
    }

    /// Is this member the result of a link?
    pub fn is_linked(&self) -> bool {
        self.key.starts_with(LINK_KEY)
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

const JIRA_KEY: &str = "jira:";
const GITHUB_KEY: &str = "github:";
const LINK_KEY: &str = "link:";

/// The key the member of a linked Jira person has (to open it right after linking).
pub fn linked_key(jira: &str) -> String {
    format!("{LINK_KEY}{jira}")
}

/// An entry counts as long as it is not done.
fn is_open(item: &Item) -> bool {
    !item.read && item.status != TicketStatus::Done
}

/// Does the entry belong to the overview right now: not done, in the chosen team, and from the
/// chosen source?
fn in_overview(ws: &Workspace, item: &Item) -> bool {
    is_open(item) && ws.team_visible(item) && ws.people_source.allows(item.source)
}

/// A review is waiting if it exists and has not been sent yet.
fn review_pending(ws: &Workspace, key: impl Into<PrKey>) -> bool {
    ws.review(key).is_some_and(|r| !r.sent)
}

/// The key and the linked GitHub login of the member an owner belongs to. Never guessed: the same
/// name in both sources stays two members unless the user linked them.
fn identity(ws: &Workspace, owner: &Person, side: Side) -> (String, Option<String>) {
    // Nobody has no name; the entries nobody owns still form one member per source.
    let name = if owner.nobody {
        crate::filters::UNASSIGNED
    } else {
        owner.name.as_str()
    };
    match side {
        Side::Jira => match ws.link_of_jira(name) {
            Some(l) => (linked_key(&l.jira), Some(l.github.clone())),
            None => (format!("{JIRA_KEY}{name}"), None),
        },
        Side::GitHub => match ws.link_of_github(name) {
            Some(l) => (linked_key(&l.jira), Some(l.github.clone())),
            None => (format!("{GITHUB_KEY}{name}"), None),
        },
    }
}

/// All people with their open entries: first „Du“ and the agent, then the
/// others in the order in which they appear in the inbox.
///
/// Jira people and GitHub users are different identities: a person appears once per source
/// (with the source chosen in [`Workspace::people_source`] only that one), and is joined with
/// another only by a link ([`crate::people_link`]). „Du“ is both: the Jira account and the
/// GitHub login are known.
///
/// The agent counts what it is currently implementing (In progress) and what it has finished and
/// is waiting for a review (Review). These tickets still belong to their owner too.
pub fn members(ws: &Workspace) -> Vec<Member> {
    let mut list = vec![Member::new(Person::me()), Member::new(Person::agent())];

    for item in ws.items.iter().filter(|i| in_overview(ws, i)) {
        let side = Side::of(item.source);
        let i = if item.owner == Person::me() {
            0
        } else if item.owner.is_agent() {
            1
        } else {
            let (key, github) = identity(ws, &item.owner, side);
            match list.iter().position(|m| m.key == key) {
                Some(i) => i,
                None => {
                    let mut member = Member::new(item.owner.clone());
                    member.key = key;
                    member.github = github;
                    if let Some(l) = ws.link_of_github(&item.owner.name)
                        && side == Side::GitHub
                    {
                        // The member carries the Jira name.
                        member.person =
                            Person::new(&l.jira, &initials_of(ws, &l.jira, &item.owner));
                    }
                    list.push(member);
                    list.len() - 1
                }
            }
        };
        let member = &mut list[i];
        member.note_side(side);
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
            .filter(|i| in_overview(ws, i))
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
            agent.note_side(Side::of(item.source));
        } else if !seen && run.is_done() && run.pr_key().is_some_and(|n| review_pending(ws, n)) {
            agent.review += 1;
            agent.item_ids.push(item.id.clone());
            agent.note_side(Side::of(item.source));
        }
    }
    list
}

/// Initials of the linked member: those of the Jira person if one of its entries is known, else
/// of the GitHub user.
fn initials_of(ws: &Workspace, jira: &str, fallback: &Person) -> String {
    ws.items
        .iter()
        .find(|i| i.source == Source::Jira && i.owner.name == jira)
        .map_or_else(|| fallback.initials.clone(), |i| i.owner.initials.clone())
}

/// The people of the list in sections. Under „Alle“ three: both sources (you, the agent, linked
/// people), Jira, GitHub; with one source chosen a single section of everybody. Empty sections
/// are left out.
pub fn sections<'a>(ws: &Workspace, list: &'a [Member]) -> Vec<(Option<Group>, Vec<&'a Member>)> {
    if ws.people_source != crate::PeopleSource::All {
        return vec![(None, list.iter().collect())];
    }
    [Group::Both, Group::Jira, Group::GitHub]
        .into_iter()
        .map(|g| (Some(g), list.iter().filter(|m| m.group() == g).collect()))
        .filter(|(_, members): &(Option<Group>, Vec<&Member>)| !members.is_empty())
        .collect()
}

/// The member the overview shows. `None` for „everybody“: no choice, or a name nobody
/// carries (the person has nothing open any more).
pub fn chosen<'a>(list: &'a [Member], name: Option<&str>) -> Option<&'a Member> {
    name.and_then(|n| list.iter().find(|m| m.key == n))
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

/// The tone of the short state of an entry in lists: for tickets the status, for pull requests
/// the state of checks and review.
pub fn state_tone(item: &Item) -> Tone {
    match (item.kind, &item.pr) {
        (Kind::PullRequest, Some(pr)) => pr.status_tone(),
        (Kind::PullRequest, None) => Tone::Gray,
        (Kind::Ticket, _) => match item.status {
            TicketStatus::Open => Tone::Gray,
            TicketStatus::InProgress => Tone::Blue,
            TicketStatus::Review => Tone::Orange,
            TicketStatus::Done => Tone::Green,
        },
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

#[cfg(test)]
mod tests;
