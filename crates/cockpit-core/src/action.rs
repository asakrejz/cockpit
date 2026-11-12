//! Actions and log.
//!
//! Every change the UI can trigger is an [`Action`]. Whoever
//! applies it – the browser in demo mode, later the backend – calls
//! [`Workspace::apply`](crate::Workspace::apply) and gets either an
//! [`Applied`] or an [`ActionError`]. Every successful action leaves
//! a [`LogEntry`] in the workspace's log; failures leave nothing.
//!
//! Actions are `serde`-capable so that the backend can receive them as JSON.

use serde::{Deserialize, Serialize};

use crate::agent::{AgentOptions, AgentReport, AgentRun, RunKind, STEPS};
use crate::attention::Rule;
use crate::demo;
use crate::model::{Checks, Item, Person, Priority, TicketStatus};
use crate::notify::{Channel, NotifyReason, QuietHours};
use crate::review::{Decision, MergeBlocker, MergeMethod, SendError, Verdict};
use crate::rules::{PathViolation, RepoRules};
use crate::sync::{self, Remote};
use crate::usage::{AgentLimits, StopReason, format_euro};
use crate::workspace::{AgentError, LinkError, ResumeError, StatusChange, Workspace};

/// This many entries the log keeps. Older ones drop out.
pub const MAX_LOG: usize = 500;

/// A change to the workspace. One variant per gesture of the UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    // Inbox
    /// Mark the entry as done (`item` is the internal ID, e.g. `bau142`).
    MarkDone {
        item: String,
    },
    SetStatus {
        item: String,
        status: TicketStatus,
    },
    CreateBranch {
        item: String,
    },
    Reply {
        item: String,
        text: String,
    },
    /// Snooze the entry. `until_ms` is the point in time (milliseconds since 1970) at
    /// which it comes back; without (`None`) it stays away until something changes: a new
    /// comment, a different status or new check results.
    Snooze {
        item: String,
        until_ms: Option<u64>,
    },
    /// Bring a snoozed entry back immediately.
    Unsnooze {
        item: String,
    },
    /// The tick: bring back entries whose snooze has expired or been lifted by a
    /// change.
    WakeSnoozed,
    /// Switch an attention rule on or off.
    SetRule {
        rule: Rule,
        enabled: bool,
    },
    // Edit tickets
    /// Change title and description of a ticket. The title must not be empty.
    EditTicket {
        item: String,
        title: String,
        description: String,
    },
    /// Assign the ticket to someone (`person` must appear in the team, see [`Workspace::people`]).
    AssignTicket {
        item: String,
        person: Person,
    },
    SetPriority {
        item: String,
        priority: Priority,
    },
    // Ticket and pull request
    /// Link the ticket (`item`) with a pull request (`pr`).
    LinkPr {
        item: String,
        pr: u32,
    },
    /// Remove the link of a ticket. The automation does not set it again.
    UnlinkPr {
        item: String,
    },
    /// The tick: link pull requests whose branch or title names a ticket key.
    AutoLinkPrs,
    /// Request a review for the pull request.
    RequestReview {
        review: u32,
    },
    /// The tick: bring the status of linked tickets in line with the pull request.
    SyncTicketStatus,
    /// Switch „status follows the pull request“ on or off.
    SetStatusFollowsPr {
        enabled: bool,
    },
    /// Choose the Jira project (key like `BAU`), `None` = all projects. In server mode the
    /// next sync then fetches all open tickets of this project.
    SelectJiraProject {
        project: Option<String>,
    },
    // Review
    Decide {
        review: u32,
        suggestion: String,
        decision: Option<Decision>,
    },
    EditSuggestion {
        review: u32,
        suggestion: String,
        text: String,
    },
    /// Own comment on a diff line, optionally with replacement code for the line.
    AddLineComment {
        review: u32,
        file: String,
        line: usize,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    /// Replaces text and suggestion; without `suggestion` the suggestion is dropped.
    EditLineComment {
        review: u32,
        comment: String,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    DeleteLineComment {
        review: u32,
        comment: String,
    },
    SetVerdict {
        review: u32,
        verdict: Verdict,
    },
    RequestAgentReview {
        review: u32,
    },
    SendReview {
        review: u32,
        comment: String,
    },
    /// Restarts a failed check (`check` is its name).
    RestartCheck {
        review: u32,
        check: String,
    },
    /// The tick of the checks: every running one advances one step.
    AdvanceChecks,
    /// Merges the pull request. Only `Actor::User` may do that.
    MergePr {
        review: u32,
        method: MergeMethod,
    },
    // Agent (`ticket` is the key, e.g. `BAU-142`)
    StartAgent {
        ticket: String,
        options: AgentOptions,
    },
    CancelAgent {
        ticket: String,
    },
    ApprovePlan {
        ticket: String,
    },
    RejectPlan {
        ticket: String,
    },
    RevisePlan {
        ticket: String,
        note: String,
    },
    /// Answer to a follow-up question of the agent (`run` is the ticket key).
    AnswerAgent {
        run: String,
        text: String,
    },
    /// Hint to the running agent that it takes into account in the next step.
    SteerAgent {
        run: String,
        text: String,
    },
    PauseAgent {
        run: String,
    },
    ResumeAgent {
        run: String,
    },
    /// The tick: every running agent completes a step.
    AdvanceAgents,
    /// Report of the real agent from the container (`run` is the key of the run). Only the
    /// backend triggers it, as `Actor::Agent`; others are rejected.
    ReportAgent {
        run: String,
        report: AgentReport,
    },
    /// Follow-up job: the agent implements comments of a review (IDs of the line comments)
    /// and pushes to the pull request's branch. The run is then called `review-<number>`.
    AddressReview {
        review: u32,
        comments: Vec<String>,
        options: AgentOptions,
    },
    /// Follow-up job: the agent investigates a red check and presents a diagnosis.
    /// The run is then called `check-<number>-<check>`.
    InvestigateCheck {
        review: u32,
        check: String,
    },
    /// Adopts the fix of the diagnosis (`run` is the key of the run): the agent pushes it
    /// to the branch and restarts the check.
    ApplyFix {
        run: String,
    },
    /// Discards the diagnosis. The run ends, the branch stays untouched.
    DiscardFix {
        run: String,
    },
    /// Resumes a stopped run after the limit that stopped it has been raised by `extra`:
    /// cents for budgets, minutes for the time limit. With `0` it continues if the
    /// limit is no longer reached at all by now (e.g. after changing the setting).
    ResumeStopped {
        run: String,
        extra: u64,
    },
    /// Set prices and limits of the agents.
    SetAgentLimits {
        limits: AgentLimits,
    },
    /// Set the rules of a repo (`repo` is its name, e.g. `owner/name`).
    SetRepoRules {
        repo: String,
        rules: RepoRules,
    },
    // Notifications
    /// Switch a channel for a reason on or off.
    SetNotifyChannel {
        reason: NotifyReason,
        channel: Channel,
        on: bool,
    },
    /// Set quiet hours (minutes since midnight, see [`QuietHours`]).
    SetQuietHours {
        quiet: QuietHours,
    },
    // Miscellaneous
    /// Back to the sample data. The log starts anew.
    ResetDemo,
    /// The sync: merges the state of Jira and GitHub with the workspace
    /// (see [`crate::sync`]). Only the system triggers it – the backend after every fetch.
    SyncRemote {
        remote: Remote,
    },
}

impl Action {
    /// Actions that only the backend itself may trigger (never the UI).
    pub fn system_only(&self) -> bool {
        matches!(self, Action::SyncRemote { .. })
    }

    /// Actions that only the agent (container, via the backend) may trigger.
    pub fn agent_only(&self) -> bool {
        matches!(self, Action::ReportAgent { .. })
    }
}

/// Who triggered the action.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    #[serde(rename = "Du")]
    User,
    Agent,
    System,
}

impl Actor {
    pub const ALL: [Actor; 3] = [Actor::User, Actor::Agent, Actor::System];

    pub fn label(self) -> &'static str {
        match self {
            Actor::User => "Du",
            Actor::Agent => "Agent",
            Actor::System => "System",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Actor::User => "du",
            Actor::Agent => "agent",
            Actor::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Option<Actor> {
        Actor::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// What a log entry refers to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An inbox entry (internal ID).
    Item(String),
    /// A review (number of the pull request).
    Review(u32),
    /// The run of an agent (ticket key).
    Run(String),
    /// The settings (notifications).
    Settings,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Running number, rises with every entry.
    pub id: u64,
    /// Point in time in milliseconds since 1970.
    pub at_ms: u64,
    pub actor: Actor,
    #[serde(default)]
    pub target: Option<Target>,
    /// Short German sentence.
    pub text: String,
}

/// Result of a successful action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// The sentence that stands in the log (with several entries: the first).
    pub summary: String,
    /// Number of new log entries.
    pub logged: usize,
}

/// Why an action was not applied. The workspace then stays unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    UnknownItem(String),
    UnknownReview(u32),
    UnknownSuggestion(String),
    UnknownComment(String),
    /// No ticket with this key.
    UnknownTicket(String),
    /// Text must not be empty, or the file and line do not exist.
    InvalidInput,
    /// Pull requests have no status.
    NotATicket,
    BranchExists,
    /// Nothing changes (already done, same status …).
    Unchanged,
    /// Nothing is running that would allow it (no waiting plan, no active run, no agent).
    NothingToDo,
    Send(SendError),
    Agent(AgentError),
    /// No check with this name.
    UnknownCheck(String),
    /// The pull request is merged and can no longer be changed.
    Merged(u32),
    /// Linking ticket and pull request is not possible.
    Link(LinkError),
    /// This person does not exist in the team.
    UnknownPerson(String),
    /// Merging happens only by hand: neither the agent nor the system may do it.
    MergeNotAllowed(Actor),
    /// As many runs are already working as the limit allows (the number).
    ParallelLimit(u32),
    /// The plan touches files that the rules of the repo do not allow.
    PlanViolatesRules(Vec<PathViolation>),
    /// Even with the raise the limit is still reached.
    LimitStillReached(StopReason),
    /// This repo does not exist.
    UnknownRepo(String),
    /// Only the system triggers this action (the sync).
    SystemOnly,
    /// There are still reasons against merging.
    MergeBlocked(Vec<MergeBlocker>),
    /// This action comes only from the agent in the container.
    AgentOnly,
}

impl ActionError {
    /// Explanation for the UI.
    pub fn message(&self) -> String {
        match self {
            ActionError::UnknownItem(id) => format!("Eintrag „{id}“ gibt es nicht"),
            ActionError::UnknownReview(n) => format!("Review #{n} gibt es nicht"),
            ActionError::UnknownSuggestion(id) => format!("Vorschlag „{id}“ gibt es nicht"),
            ActionError::UnknownComment(id) => format!("Kommentar „{id}“ gibt es nicht"),
            ActionError::UnknownTicket(key) => format!("Ticket „{key}“ gibt es nicht"),
            ActionError::InvalidInput => "Die Eingabe passt nicht".into(),
            ActionError::NotATicket => "Pull Requests haben keinen Status".into(),
            ActionError::BranchExists => "Es gibt schon einen Branch".into(),
            ActionError::Unchanged => "Es ändert sich nichts".into(),
            ActionError::NothingToDo => "Dafür gibt es gerade nichts zu tun".into(),
            ActionError::Send(SendError::Undecided(n)) => {
                format!("{n} Vorschläge sind noch nicht entschieden")
            }
            ActionError::Send(SendError::AlreadySent) => "Das Review ist schon gesendet".into(),
            ActionError::Send(SendError::UnknownReview) => "Das Review gibt es nicht".into(),
            ActionError::Agent(AgentError::UnknownTicket) => "Dieses Ticket gibt es nicht".into(),
            ActionError::Agent(AgentError::AlreadyRunning) => {
                "Der Agent arbeitet hier schon".into()
            }
            ActionError::Agent(AgentError::UnknownReview) => "Das Review gibt es nicht".into(),
            ActionError::Agent(AgentError::Merged) => {
                "Der Pull Request ist schon gemergt und bleibt, wie er ist".into()
            }
            ActionError::Agent(AgentError::NoComments) => {
                "Wähle mindestens einen offenen Kommentar aus".into()
            }
            ActionError::Agent(AgentError::CheckNotFailed) => {
                "Untersuchen lässt sich nur ein roter Check".into()
            }
            ActionError::Agent(AgentError::ParallelLimit(n)) | ActionError::ParallelLimit(n) => {
                format!(
                    "Es arbeiten schon {n} Agenten – mehr erlaubt die Grenze nicht. Warte, bis einer fertig ist, oder erhöhe die Grenze unter „Grenzen & Regeln“."
                )
            }
            ActionError::PlanViolatesRules(violations) => {
                let reasons: Vec<String> = violations.iter().map(PathViolation::reason).collect();
                format!(
                    "Der Plan lässt sich nicht freigeben: {}. Ändere die Regeln des Repos oder lehne den Plan ab.",
                    reasons.join("; ")
                )
            }
            ActionError::LimitStillReached(reason) => format!(
                "{} – auch mit dieser Erhöhung. Erhöhe um mehr.",
                reason.label()
            ),
            ActionError::UnknownRepo(repo) => format!("Repo „{repo}“ gibt es nicht"),
            ActionError::UnknownCheck(name) => format!("Check „{name}“ gibt es nicht"),
            ActionError::UnknownPerson(name) => format!("{name} gibt es im Team nicht"),
            ActionError::Link(LinkError::UnknownPr(n)) => {
                format!("Pull Request #{n} gibt es nicht")
            }
            ActionError::Link(LinkError::Same) => "Beide sind schon verknüpft".into(),
            ActionError::Link(LinkError::TicketHasPr(n)) => {
                format!("Das Ticket hängt schon an #{n} – löse das zuerst")
            }
            ActionError::Link(LinkError::PrHasTicket(key)) => {
                format!("Der Pull Request gehört schon zu {key}")
            }
            ActionError::Merged(n) => format!("#{n} ist schon gemergt und bleibt, wie es ist"),
            ActionError::MergeNotAllowed(actor) => format!(
                "Nur du darfst mergen – {} nicht",
                match actor {
                    Actor::Agent => "der Agent",
                    Actor::System => "das System",
                    Actor::User => "du",
                }
            ),
            ActionError::SystemOnly => {
                "Das macht nur das System – der Abgleich läuft im Server".into()
            }
            ActionError::AgentOnly => "Das meldet nur der Agent aus seinem Container".into(),
            ActionError::MergeBlocked(blockers) => {
                let reasons: Vec<&str> = blockers.iter().map(|b| b.reason()).collect();
                format!("Mergen geht noch nicht: {}", reasons.join(" "))
            }
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ActionError {}

/// An entry that an action wants to write into the log.
struct Note {
    actor: Option<Actor>,
    target: Option<Target>,
    text: String,
}

impl Note {
    fn new(target: Target, text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target: Some(target),
            text: text.into(),
        }
    }

    /// An entry without a specific target (settings).
    fn general(text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target: None,
            text: text.into(),
        }
    }

    fn by(mut self, actor: Actor) -> Self {
        self.actor = Some(actor);
        self
    }
}

/// Error when commissioning: the limit of parallel runs has its own message.
fn agent_error(e: AgentError) -> ActionError {
    match e {
        AgentError::ParallelLimit(n) => ActionError::ParallelLimit(n),
        other => ActionError::Agent(other),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn status_label(status: TicketStatus) -> &'static str {
    TicketStatus::OPTIONS
        .iter()
        .find(|(id, _)| *id == status.id())
        .map_or("", |(_, label)| label)
}

/// Log entries of the system for status changes that the pull request triggered.
fn status_notes(changes: Vec<StatusChange>) -> Vec<Note> {
    changes
        .into_iter()
        .map(|c| {
            let why = match c.to {
                TicketStatus::Done => format!("#{} ist gemergt", c.pr),
                TicketStatus::Review => format!("für #{} ist ein Review angefragt", c.pr),
                _ => format!("#{} ist noch in Arbeit", c.pr),
            };
            Note::new(
                Target::Item(c.item),
                format!(
                    "Status von {} automatisch auf „{}“ gesetzt – {why}",
                    c.key,
                    status_label(c.to)
                ),
            )
            .by(Actor::System)
        })
        .collect()
}

impl Workspace {
    /// Applies an action and writes it into the log.
    ///
    /// `actor` is who triggered it, `at_ms` the point in time (milliseconds since 1970;
    /// the time comes from outside so that the logic stays testable). On an error
    /// workspace and log stay unchanged.
    pub fn apply(
        &mut self,
        action: Action,
        actor: Actor,
        at_ms: u64,
    ) -> Result<Applied, ActionError> {
        // Merging is the user's business: the layer already checks this, not only the UI.
        if matches!(action, Action::MergePr { .. }) && actor != Actor::User {
            return Err(ActionError::MergeNotAllowed(actor));
        }
        if action.system_only() && actor != Actor::System {
            return Err(ActionError::SystemOnly);
        }
        if action.agent_only() && actor != Actor::Agent {
            return Err(ActionError::AgentOnly);
        }
        let notes = self.perform(action, at_ms)?;
        let mut summary = String::new();
        let logged = notes.len();
        for note in notes {
            if summary.is_empty() {
                summary = note.text.clone();
            }
            self.push_log(at_ms, note.actor.unwrap_or(actor), note.target, note.text);
        }
        Ok(Applied { summary, logged })
    }

    fn push_log(&mut self, at_ms: u64, actor: Actor, target: Option<Target>, text: String) {
        let id = self.log.last().map_or(1, |e| e.id + 1);
        self.log.push(LogEntry {
            id,
            at_ms,
            actor,
            target,
            text,
        });
        if self.log.len() > MAX_LOG {
            let excess = self.log.len() - MAX_LOG;
            self.log.drain(..excess);
        }
    }

    /// The log, newest entries first. With `actor` only that actor's entries.
    pub fn log_entries(&self, actor: Option<Actor>) -> Vec<&LogEntry> {
        self.log
            .iter()
            .rev()
            .filter(|e| actor.is_none_or(|a| e.actor == a))
            .collect()
    }

    fn key_of(&self, id: &str) -> Result<String, ActionError> {
        self.item(id)
            .map(|i| i.key.clone())
            .ok_or_else(|| ActionError::UnknownItem(id.to_string()))
    }

    fn need_review(&self, number: u32) -> Result<(), ActionError> {
        self.review(number)
            .map(|_| ())
            .ok_or(ActionError::UnknownReview(number))
    }

    /// Like `need_review`, but a merged pull request can no longer be changed.
    fn need_open_review(&self, number: u32) -> Result<(), ActionError> {
        match self.review(number) {
            None => Err(ActionError::UnknownReview(number)),
            Some(r) if r.merged.is_some() => Err(ActionError::Merged(number)),
            Some(_) => Ok(()),
        }
    }

    /// Executes the action and returns the log entries for it.
    fn perform(&mut self, action: Action, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::MarkDone { item } => {
                let key = self.key_of(&item)?;
                if self.item(&item).is_some_and(|i| i.read) {
                    return Err(ActionError::Unchanged);
                }
                self.mark_done(&item);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{key} als erledigt markiert"),
                )])
            }
            Action::SetStatus { item, status } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).map(|i| (i.is_ticket(), i.status));
                match current {
                    Some((false, _)) => return Err(ActionError::NotATicket),
                    Some((_, now)) if now == status => return Err(ActionError::Unchanged),
                    _ => {}
                }
                self.set_status(&item, status);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Status von {key} auf „{}“ gesetzt", status_label(status)),
                )])
            }
            Action::CreateBranch { item } => {
                let key = self.key_of(&item)?;
                let name = self.create_branch(&item).ok_or(ActionError::BranchExists)?;
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Branch {name} für {key} erstellt"),
                )])
            }
            Action::Reply { item, text } => {
                let key = self.key_of(&item)?;
                if !self.reply(&item, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Antwort an {key} gesendet"),
                )])
            }
            Action::Snooze { item, until_ms } => {
                let key = self.key_of(&item)?;
                if until_ms.is_some_and(|t| t <= at_ms) {
                    return Err(ActionError::InvalidInput);
                }
                self.snooze(&item, until_ms);
                let text = match until_ms {
                    Some(_) => format!("{key} zurückgestellt"),
                    None => format!("{key} zurückgestellt, bis sich etwas ändert"),
                };
                Ok(vec![Note::new(Target::Item(item), text)])
            }
            Action::Unsnooze { item } => {
                let key = self.key_of(&item)?;
                if !self.unsnooze(&item) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{key} aus der Zurückstellung geholt"),
                )])
            }
            Action::WakeSnoozed => {
                let back = self.wake(at_ms);
                if back.is_empty() {
                    return Err(ActionError::NothingToDo);
                }
                Ok(back
                    .into_iter()
                    .map(|id| {
                        let key = self.item(&id).map_or(id.clone(), |i| i.key.clone());
                        Note::new(Target::Item(id), format!("{key} ist zurück in der Inbox"))
                            .by(Actor::System)
                    })
                    .collect())
            }
            Action::SetRule { rule, enabled } => {
                if !self.set_rule(rule, enabled) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::general(format!(
                    "Regel „{}“ {}",
                    rule.label(),
                    if enabled {
                        "eingeschaltet"
                    } else {
                        "ausgeschaltet"
                    }
                ))])
            }
            Action::EditTicket {
                item,
                title,
                description,
            } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of kennt den Eintrag");
                if !current.is_ticket() {
                    return Err(ActionError::NotATicket);
                }
                if title.trim().is_empty() {
                    return Err(ActionError::InvalidInput);
                }
                let title_changed = current.title != title.trim();
                let description_changed = current.description != description.trim();
                if !title_changed && !description_changed {
                    return Err(ActionError::Unchanged);
                }
                self.edit_ticket(&item, &title, &description);
                let what = match (title_changed, description_changed) {
                    (true, true) => "Titel und Beschreibung",
                    (true, false) => "Titel",
                    _ => "Beschreibung",
                };
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{what} von {key} geändert"),
                )])
            }
            Action::AssignTicket { item, person } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of kennt den Eintrag");
                if !current.is_ticket() {
                    return Err(ActionError::NotATicket);
                }
                if !self.people().contains(&person) {
                    return Err(ActionError::UnknownPerson(person.name));
                }
                if current.owner == person {
                    return Err(ActionError::Unchanged);
                }
                self.assign(&item, &person);
                let text = if person == Person::me() {
                    format!("{key} dir zugewiesen")
                } else {
                    format!("{key} an {} zugewiesen", person.name)
                };
                Ok(vec![Note::new(Target::Item(item), text)])
            }
            Action::SetPriority { item, priority } => {
                let key = self.key_of(&item)?;
                match self.item(&item) {
                    Some(i) if !i.is_ticket() => return Err(ActionError::NotATicket),
                    Some(i) if i.priority == priority => return Err(ActionError::Unchanged),
                    _ => {}
                }
                self.set_priority(&item, priority);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Priorität von {key} auf „{}“ gesetzt", priority.label()),
                )])
            }
            Action::LinkPr { item, pr } => {
                let key = self.key_of(&item)?;
                if !self.item(&item).is_some_and(Item::is_ticket) {
                    return Err(ActionError::NotATicket);
                }
                self.link_pr(&item, pr).map_err(|e| match e {
                    LinkError::Same => ActionError::Unchanged,
                    e => ActionError::Link(e),
                })?;
                let mut notes = vec![Note::new(
                    Target::Item(item),
                    format!("{key} mit #{pr} verknüpft"),
                )];
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::UnlinkPr { item } => {
                let key = self.key_of(&item)?;
                if !self.item(&item).is_some_and(Item::is_ticket) {
                    return Err(ActionError::NotATicket);
                }
                let number = self.unlink_pr(&item).ok_or(ActionError::NothingToDo)?;
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Verknüpfung von {key} mit #{number} gelöst"),
                )])
            }
            Action::AutoLinkPrs => {
                let linked = self.auto_link();
                if linked.is_empty() {
                    return Err(ActionError::NothingToDo);
                }
                let mut notes: Vec<Note> = linked
                    .into_iter()
                    .map(|(id, key, number)| {
                        Note::new(
                            Target::Item(id),
                            format!("{key} automatisch mit #{number} verknüpft"),
                        )
                        .by(Actor::System)
                    })
                    .collect();
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::RequestReview { review } => {
                self.need_open_review(review)?;
                if !self.request_review(review) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!("Review für #{review} angefragt"),
                )];
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::SyncTicketStatus => {
                // An error changes nothing: first check whether a pull request has moved.
                if !self.status_pending() {
                    return Err(ActionError::NothingToDo);
                }
                // If the status already fits, the state is only noted – without an entry.
                Ok(status_notes(self.follow_status()))
            }
            Action::SetStatusFollowsPr { enabled } => {
                if !self.set_status_follows_pr(enabled) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(
                    Target::Settings,
                    format!(
                        "Status folgt dem Pull Request: {}",
                        if enabled {
                            "eingeschaltet"
                        } else {
                            "ausgeschaltet"
                        }
                    ),
                )];
                // Switched on again: whatever has moved in the meantime is brought in line.
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::SelectJiraProject { project } => {
                let project = project
                    .map(|p| p.trim().to_uppercase())
                    .filter(|p| !p.is_empty());
                if project
                    .as_deref()
                    .is_some_and(|p| !crate::is_project_key(p))
                {
                    return Err(ActionError::InvalidInput);
                }
                let text = match &project {
                    Some(p) => format!("Jira-Projekt gewählt: {p}"),
                    None => "Jira-Projekt: alle".to_string(),
                };
                if !self.set_jira_project(project) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::new(Target::Settings, text)])
            }
            Action::Decide {
                review,
                suggestion,
                decision,
            } => {
                self.need_open_review(review)?;
                if !self.decide(review, &suggestion, decision) {
                    return Err(ActionError::UnknownSuggestion(suggestion));
                }
                let text = match decision {
                    Some(Decision::Approved) => {
                        format!("Vorschlag {suggestion} in #{review} freigegeben")
                    }
                    Some(Decision::Declined) => {
                        format!("Vorschlag {suggestion} in #{review} abgelehnt")
                    }
                    None => format!(
                        "Entscheidung zu Vorschlag {suggestion} in #{review} zurückgenommen"
                    ),
                };
                Ok(vec![Note::new(Target::Review(review), text)])
            }
            Action::EditSuggestion {
                review,
                suggestion,
                text,
            } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.suggestions.iter().any(|s| s.id == suggestion));
                if !known {
                    return Err(ActionError::UnknownSuggestion(suggestion));
                }
                if !self.edit_suggestion(review, &suggestion, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Vorschlag {suggestion} in #{review} bearbeitet"),
                )])
            }
            Action::AddLineComment {
                review,
                file,
                line,
                text,
                suggestion,
            } => {
                self.need_open_review(review)?;
                if self
                    .add_line_comment(review, &file, line, &text, suggestion.as_deref())
                    .is_none()
                {
                    return Err(ActionError::InvalidInput);
                }
                let with = if suggestion.is_some() {
                    " mit Änderungsvorschlag"
                } else {
                    ""
                };
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Kommentar{with} an {file}, Zeile {}, in #{review} hinzugefügt",
                        line + 1
                    ),
                )])
            }
            Action::EditLineComment {
                review,
                comment,
                text,
                suggestion,
            } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.line_comments.iter().any(|c| c.id == comment));
                if !known {
                    return Err(ActionError::UnknownComment(comment));
                }
                if !self.edit_line_comment(review, &comment, &text, suggestion.as_deref()) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Eigenen Kommentar in #{review} bearbeitet"),
                )])
            }
            Action::DeleteLineComment { review, comment } => {
                self.need_open_review(review)?;
                if !self.delete_line_comment(review, &comment) {
                    return Err(ActionError::UnknownComment(comment));
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Eigenen Kommentar in #{review} gelöscht"),
                )])
            }
            Action::SetVerdict { review, verdict } => {
                self.need_open_review(review)?;
                if self.review(review).is_some_and(|r| r.verdict == verdict) {
                    return Err(ActionError::Unchanged);
                }
                self.set_verdict(review, verdict);
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Ergebnis von #{review} auf „{}“ gesetzt", verdict.label()),
                )])
            }
            Action::RequestAgentReview { review } => {
                self.need_open_review(review)?;
                let count = self.request_agent_review(review);
                let result = match count {
                    0 => "Nichts zu beanstanden".to_string(),
                    n => format!("{} vorbereitet", plural(n, "Kommentar", "Kommentare")),
                };
                Ok(vec![
                    Note::new(
                        Target::Review(review),
                        format!("Agent-Review für #{review} angefordert"),
                    ),
                    Note::new(
                        Target::Review(review),
                        format!("#{review} durchgesehen: {result}"),
                    )
                    .by(Actor::Agent),
                ])
            }
            Action::SendReview { review, comment } => {
                let count = self.send_review(review, &comment).map_err(|e| match e {
                    SendError::UnknownReview => ActionError::UnknownReview(review),
                    e => ActionError::Send(e),
                })?;
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Review für #{review} gesendet ({})",
                        plural(count, "Kommentar", "Kommentare")
                    ),
                )])
            }
            Action::RestartCheck { review, check } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.check_runs.iter().any(|c| c.name == check));
                if !known {
                    return Err(ActionError::UnknownCheck(check));
                }
                if !self.restart_check(review, &check) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Check „{check}“ in #{review} neu gestartet"),
                )])
            }
            Action::AdvanceChecks => {
                if self.running_checks() == 0 {
                    return Err(ActionError::NothingToDo);
                }
                Ok(self
                    .advance_checks()
                    .into_iter()
                    .map(|(number, check, status)| {
                        let result = if status == Checks::Passed {
                            "bestanden"
                        } else {
                            "wieder fehlgeschlagen"
                        };
                        Note::new(
                            Target::Review(number),
                            format!("Check „{check}“ in #{number} {result}"),
                        )
                        .by(Actor::System)
                    })
                    .collect())
            }
            Action::MergePr { review, method } => {
                self.need_review(review)?;
                let changes = self
                    .merge_pr(review, method)
                    .map_err(ActionError::MergeBlocked)?;
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!("#{review} gemergt ({})", method.label()),
                )];
                notes.extend(status_notes(changes));
                Ok(notes)
            }
            Action::StartAgent { ticket, options } => {
                self.start_agent(&ticket, options).map_err(agent_error)?;
                // The rules of the repo can force a plan, even if none was requested.
                let plan_first = self
                    .runs
                    .iter()
                    .any(|r| r.key() == ticket && r.is_awaiting());
                let mut notes = vec![Note::new(
                    Target::Run(ticket.clone()),
                    format!("Agent für {ticket} beauftragt"),
                )];
                if plan_first {
                    notes.push(
                        Note::new(
                            Target::Run(ticket.clone()),
                            format!("Plan für {ticket} vorgelegt, wartet auf Freigabe"),
                        )
                        .by(Actor::Agent),
                    );
                }
                Ok(notes)
            }
            Action::CancelAgent { ticket } => {
                let Some(label) = self
                    .runs
                    .iter()
                    .find(|r| r.key() == ticket && r.is_active())
                    .map(AgentRun::label)
                else {
                    return Err(ActionError::NothingToDo);
                };
                self.cancel_agent(&ticket);
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Lauf des Agenten für {label} abgebrochen"),
                )])
            }
            Action::ApprovePlan { ticket } => {
                let label = self.label_of(&ticket);
                if let Some(run) = self
                    .runs
                    .iter()
                    .find(|r| r.key() == ticket && r.is_awaiting())
                {
                    let violations = run.violations();
                    if !violations.is_empty() {
                        return Err(ActionError::PlanViolatesRules(violations));
                    }
                    if let Some(max) = self.parallel_full() {
                        return Err(ActionError::ParallelLimit(max));
                    }
                }
                if !self.approve_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Plan für {label} freigegeben"),
                )])
            }
            Action::RejectPlan { ticket } => {
                let label = self.label_of(&ticket);
                if !self.reject_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Plan für {label} abgelehnt"),
                )])
            }
            Action::RevisePlan { ticket, note } => {
                let waiting = self
                    .runs
                    .iter()
                    .any(|r| r.key() == ticket && r.is_awaiting());
                if !waiting {
                    return Err(ActionError::NothingToDo);
                }
                let label = self.label_of(&ticket);
                if !self.revise_plan(&ticket, &note) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Neuen Plan für {label} angefordert"),
                )])
            }
            Action::SetNotifyChannel {
                reason,
                channel,
                on,
            } => {
                if !self.notifications.set(reason, channel, on) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::new(
                    Target::Settings,
                    format!(
                        "Benachrichtigung „{}“ über {} {}",
                        reason.label(),
                        channel.label(),
                        if on { "eingeschaltet" } else { "ausgeschaltet" }
                    ),
                )])
            }
            Action::SetQuietHours { quiet } => {
                if !quiet.is_valid() {
                    return Err(ActionError::InvalidInput);
                }
                if !self.notifications.set_quiet(quiet) {
                    return Err(ActionError::Unchanged);
                }
                let text = if quiet.enabled {
                    format!(
                        "Ruhezeiten von {} bis {} Uhr eingeschaltet",
                        QuietHours::format(quiet.from),
                        QuietHours::format(quiet.to)
                    )
                } else {
                    "Ruhezeiten ausgeschaltet".to_string()
                };
                Ok(vec![Note::new(Target::Settings, text)])
            }
            Action::AnswerAgent { run, text } => {
                if !self.runs.iter().any(|r| r.key() == run && r.is_asking()) {
                    return Err(ActionError::NothingToDo);
                }
                let label = self.label_of(&run);
                if !self.answer_agent(&run, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!(
                        "Frage des Agenten für {label} beantwortet: „{}“",
                        text.trim()
                    ),
                )])
            }
            Action::SteerAgent { run, text } => {
                if !self
                    .runs
                    .iter()
                    .any(|r| r.key() == run && r.is_active() && !r.is_awaiting())
                {
                    return Err(ActionError::NothingToDo);
                }
                let label = self.label_of(&run);
                if !self.steer_agent(&run, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Hinweis an den Agenten für {label}: „{}“", text.trim()),
                )])
            }
            Action::PauseAgent { run } => {
                let label = self.label_of(&run);
                if !self.pause_agent(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Agent für {label} pausiert"),
                )])
            }
            Action::ResumeAgent { run } => {
                let label = self.label_of(&run);
                if self.runs.iter().any(|r| r.key() == run && r.is_paused())
                    && let Some(max) = self.parallel_full()
                {
                    return Err(ActionError::ParallelLimit(max));
                }
                if !self.resume_agent(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Agent für {label} fortgesetzt"),
                )])
            }
            Action::AdvanceAgents => self.advance_logged(at_ms),
            Action::ReportAgent { run, report } => {
                let effect = self
                    .report_agent(&run, report, at_ms)
                    .ok_or(ActionError::NothingToDo)?;
                let label = self.label_of(&run);
                let mut notes: Vec<Note> = effect
                    .notes
                    .into_iter()
                    .map(|text| Note::new(Target::Run(run.clone()), text).by(Actor::Agent))
                    .collect();
                if effect.finished {
                    notes.push(
                        Note::new(
                            Target::Run(run.clone()),
                            format!(
                                "{label}: Fertig – Branch gepusht, der Pull Request wartet auf dich"
                            ),
                        )
                        .by(Actor::Agent),
                    );
                }
                // Messages without a sentence (file list, hints delivered) are not a log entry.
                Ok(notes)
            }
            Action::AddressReview {
                review,
                comments,
                options,
            } => {
                self.need_open_review(review)?;
                // Unknown comments report by name, done ones count like missing ones.
                if let Some(id) = comments.iter().find(|id| {
                    self.review(review)
                        .is_some_and(|r| !r.line_comments.iter().any(|c| &c.id == *id))
                }) {
                    return Err(ActionError::UnknownComment(id.clone()));
                }
                let plan_first = options.plan_first;
                let n = comments.len();
                let key = self
                    .address_review(review, &comments, options)
                    .map_err(ActionError::Agent)?;
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Agent beauftragt, {} aus #{review} umzusetzen",
                        plural(n, "Kommentar", "Kommentare")
                    ),
                )];
                if plan_first {
                    notes.push(
                        Note::new(
                            Target::Run(key),
                            format!("Plan für die Kommentare in #{review} vorgelegt, wartet auf Freigabe"),
                        )
                        .by(Actor::Agent),
                    );
                }
                Ok(notes)
            }
            Action::InvestigateCheck { review, check } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.check_runs.iter().any(|c| c.name == check));
                if !known {
                    return Err(ActionError::UnknownCheck(check));
                }
                self.investigate_check(review, &check)
                    .map_err(ActionError::Agent)?;
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Agent beauftragt, den Check „{check}“ in #{review} zu untersuchen"),
                )])
            }
            Action::ApplyFix { run } => {
                let label = self.label_of(&run);
                let pushes = self
                    .runs
                    .iter()
                    .find(|r| r.key() == run && r.is_diagnosed())
                    .and_then(|r| r.diagnosis.as_ref())
                    .is_none_or(|d| d.changes_code());
                if !self.accept_fix(&run) {
                    return Err(ActionError::NothingToDo);
                }
                let text = if pushes {
                    format!(
                        "Fix für {label} übernommen: Der Agent pusht ihn und startet den Check neu"
                    )
                } else {
                    format!("Vorschlag für {label} übernommen: Der Agent startet den Check neu")
                };
                Ok(vec![Note::new(Target::Run(run), text)])
            }
            Action::DiscardFix { run } => {
                let label = self.label_of(&run);
                if !self.discard_fix(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Diagnose für {label} verworfen, am Branch ändert sich nichts"),
                )])
            }
            Action::ResumeStopped { run, extra } => {
                let reason = self
                    .resume_stopped(&run, extra, at_ms)
                    .map_err(|e| match e {
                        ResumeError::NotStopped => ActionError::NothingToDo,
                        ResumeError::ParallelLimit(n) => ActionError::ParallelLimit(n),
                        ResumeError::StillReached(r) => ActionError::LimitStillReached(r),
                    })?;
                let text = if extra == 0 {
                    format!("Agent für {run} fortgesetzt")
                } else {
                    format!(
                        "Agent für {run} fortgesetzt – {} um {} erhöht",
                        reason.raises(),
                        reason.raise_label(extra)
                    )
                };
                Ok(vec![Note::new(Target::Run(run.clone()), text)])
            }
            Action::SetAgentLimits { limits } => {
                if !limits.is_valid() {
                    return Err(ActionError::InvalidInput);
                }
                let text = format!("Grenzen für Agenten geändert: {}", limits.summary());
                let pricing_changed = limits.pricing != self.limits.pricing;
                if !self.set_limits(limits) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(Target::Settings, text)];
                if pricing_changed {
                    notes.push(Note::new(
                        Target::Settings,
                        format!(
                            "Preise geändert: {} je Million Token ein, {} aus",
                            format_euro(self.limits.pricing.input_cents_per_mtok),
                            format_euro(self.limits.pricing.output_cents_per_mtok)
                        ),
                    ));
                }
                Ok(notes)
            }
            Action::SetRepoRules { repo, rules } => {
                if !self.repos().contains(&repo) {
                    return Err(ActionError::UnknownRepo(repo));
                }
                if !self.set_repo_rules(&repo, rules) {
                    return Err(ActionError::Unchanged);
                }
                let lines = self.rules_for(&repo).lines();
                let text = if lines.is_empty() {
                    format!("Regeln für {repo} entfernt")
                } else {
                    format!("Regeln für {repo} geändert: {}", lines.join(" · "))
                };
                Ok(vec![Note::new(Target::Settings, text)])
            }
            Action::SyncRemote { remote } => {
                let (merged, changes) = sync::merge(self, &remote, at_ms);
                *self = merged;
                let mut notes: Vec<Note> = changes
                    .into_iter()
                    .map(|c| Note {
                        actor: Some(Actor::System),
                        target: c.target(),
                        text: c.text(),
                    })
                    .collect();
                // The sentence in the result comes from the first change; without a change it stays empty.
                notes.truncate(MAX_LOG);
                Ok(notes)
            }
            Action::ResetDemo => {
                // The settings do not belong to the sample data and stay.
                let notifications = std::mem::take(&mut self.notifications);
                let sources = std::mem::take(&mut self.sync.sources);
                let pr_status_off = self.pr_status_off;
                let limits = self.limits.clone();
                let repo_rules = std::mem::take(&mut self.repo_rules);
                let jira_project = self.jira_project.take();
                *self = demo::workspace();
                self.jira_project = jira_project;
                self.notifications = notifications;
                self.pr_status_off = pr_status_off;
                self.limits = limits;
                self.repo_rules = repo_rules;
                self.sync.sources = sources;
                Ok(vec![
                    Note::new(
                        Target::Item(self.items.first().map(|i| i.id.clone()).unwrap_or_default()),
                        "Demo-Daten zurückgesetzt",
                    )
                    .by(Actor::System),
                ])
            }
        }
    }

    /// The name of the run for log sentences: the ticket key, for follow-up jobs with
    /// plain text. An unknown key stays as it is.
    fn label_of(&self, key: &str) -> String {
        self.runs
            .iter()
            .find(|r| r.key() == key)
            .map_or_else(|| key.to_string(), AgentRun::label)
    }

    /// The tick of the agents, with one entry per completed step or asked question.
    fn advance_logged(&mut self, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        // Note beforehand which step is about to finish and which hints are waiting.
        let stepping: Vec<(String, usize, String, Option<u32>, bool, usize)> = self
            .runs
            .iter()
            .filter(|r| r.is_running() && !r.real)
            .map(|r| {
                let title = r.steps()[r.completed.min(STEPS - 1)].title.clone();
                (
                    r.key(),
                    r.completed,
                    title,
                    r.pr_number,
                    r.options.draft_pr,
                    r.pending_hints.len(),
                )
            })
            .collect();
        if stepping.is_empty() {
            return Err(ActionError::NothingToDo);
        }
        self.advance_agents_at(at_ms);
        let mut notes = Vec::new();
        for (key, done, title, pr, draft, hints) in stepping {
            let Some(run) = self.runs.iter().find(|r| r.key() == key) else {
                continue;
            };
            let label = run.label();
            let (asking, diagnosed) = (run.is_asking(), run.is_diagnosed());
            let kind = run.kind.clone();
            if hints > 0 {
                notes.push(
                    Note::new(
                        Target::Run(key.clone()),
                        format!(
                            "{label}: {} berücksichtigt",
                            plural(hints, "Hinweis", "Hinweise")
                        ),
                    )
                    .by(Actor::Agent),
                );
            }
            // A limit stopped the run before it did the step.
            if let Some(reason) = run.stop_reason() {
                notes.push(Note::new(
                    Target::Run(key.clone()),
                    format!("{label}: Angehalten – {}", reason.label()),
                ));
                continue;
            }
            let last = done + 1 == STEPS;
            notes.push(match (asking, diagnosed, last, pr, kind) {
                (true, ..) => Note::new(
                    Target::Run(key.clone()),
                    format!("{label}: Der Agent hat eine Frage und wartet auf deine Antwort"),
                ),
                (_, true, ..) => Note::new(
                    Target::Run(key.clone()),
                    format!("{label}: Diagnose liegt vor, der Agent wartet auf deine Entscheidung"),
                ),
                (_, _, true, Some(n), RunKind::AddressReview { comments, .. }) => Note::new(
                    Target::Review(n),
                    format!(
                        "#{n}: {} umgesetzt und auf den Branch gepusht, das Review wartet wieder auf dich",
                        plural(comments.len(), "Kommentar", "Kommentare")
                    ),
                ),
                (_, _, true, Some(n), RunKind::Diagnose { check, .. }) => Note::new(
                    Target::Review(n),
                    format!("#{n}: Check „{check}“ nach dem Fix neu gestartet"),
                ),
                (_, _, true, Some(n), RunKind::Implement) => Note::new(
                    Target::Review(n),
                    format!(
                        "{key}: Pull Request #{n} {}erstellt",
                        if draft { "als Entwurf " } else { "" }
                    ),
                ),
                _ => Note::new(Target::Run(key.clone()), format!("{label}: {title}")),
            });
        }
        Ok(notes)
    }
}

#[cfg(test)]
mod tests;
