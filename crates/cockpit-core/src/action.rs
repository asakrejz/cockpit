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

use crate::agent::{AgentOptions, AgentReport, AgentRun, PullRequestRef, RunKind, STEPS};
use crate::attention::Rule;
use crate::demo;
use crate::model::{Checks, Item, Person, PrKey, Priority, TicketStatus};
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
        pr: PrKey,
    },
    /// Remove the link of a ticket. The automation does not set it again.
    UnlinkPr {
        item: String,
    },
    /// The tick: link pull requests whose branch or title names a ticket key.
    AutoLinkPrs,
    /// Request a review for the pull request.
    RequestReview {
        review: PrKey,
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
        review: PrKey,
        suggestion: String,
        decision: Option<Decision>,
    },
    EditSuggestion {
        review: PrKey,
        suggestion: String,
        text: String,
    },
    /// Own comment on a diff line, optionally with replacement code for the line.
    AddLineComment {
        review: PrKey,
        file: String,
        line: usize,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    /// Replaces text and suggestion; without `suggestion` the suggestion is dropped.
    EditLineComment {
        review: PrKey,
        comment: String,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    DeleteLineComment {
        review: PrKey,
        comment: String,
    },
    SetVerdict {
        review: PrKey,
        verdict: Verdict,
    },
    RequestAgentReview {
        review: PrKey,
    },
    SendReview {
        review: PrKey,
        comment: String,
    },
    /// Restarts a failed check (`check` is its name).
    RestartCheck {
        review: PrKey,
        check: String,
    },
    /// The tick of the checks: every running one advances one step.
    AdvanceChecks,
    /// Merges the pull request. Only `Actor::User` may do that.
    MergePr {
        review: PrKey,
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
        review: PrKey,
        comments: Vec<String>,
        options: AgentOptions,
    },
    /// Follow-up job: the agent investigates a red check and presents a diagnosis.
    /// The run is then called `check-<number>-<check>`.
    InvestigateCheck {
        review: PrKey,
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
    /// Follow-up job of the **real** agent: implement the review comments (threads) of the
    /// pull request. The backend reads them from GitHub; the agent continues on the same `claude/…`
    /// branch. The run is then called `threads-<number>`.
    AddressReviewThreads {
        review: PrKey,
        options: AgentOptions,
    },
    /// Follow-up job of the **real** agent: fix the red checks of the pull request (names and a
    /// shortened log excerpt go to the agent). The run is then called `fixes-<number>`.
    FixFailedChecks {
        review: PrKey,
        options: AgentOptions,
    },
    /// Creates the pull request for the pushed branch of a finished real run – only on your explicit
    /// approval, with the title and text you saw in the preview. `pull` is filled in by the backend
    /// after GitHub has created the pull request (or found the existing one); a value sent by the
    /// browser is dropped.
    CreatePullRequest {
        run: String,
        title: String,
        body: String,
        #[serde(default)]
        pull: Option<PullRequestRef>,
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

    /// Actions with an effect on GitHub or on the agent's work that only you may trigger – neither
    /// the agent nor the system.
    pub fn user_only(&self) -> bool {
        matches!(
            self,
            Action::AddressReviewThreads { .. }
                | Action::FixFailedChecks { .. }
                | Action::CreatePullRequest { .. }
        )
    }
}

mod agent;
mod error;
mod inbox;
mod note;
mod review;
mod settings;
#[cfg(test)]
mod tests;

pub use error::ActionError;
use error::agent_error;
use note::{Note, plural, status_label, status_notes};

mod log;
pub use log::*;

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
        if action.user_only() && actor != Actor::User {
            return Err(ActionError::UserOnly(actor));
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

    fn need_review(&self, number: &PrKey) -> Result<(), ActionError> {
        self.review(number)
            .map(|_| ())
            .ok_or_else(|| ActionError::UnknownReview(number.clone()))
    }

    /// Like `need_review`, but a merged pull request can no longer be changed.
    fn need_open_review(&self, number: &PrKey) -> Result<(), ActionError> {
        match self.review(number) {
            None => Err(ActionError::UnknownReview(number.clone())),
            Some(r) if r.merged.is_some() => Err(ActionError::Merged(r.key())),
            Some(_) => Ok(()),
        }
    }

    /// Executes the action and returns the log entries for it.
    fn perform(&mut self, action: Action, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::MarkDone { .. }
            | Action::SetStatus { .. }
            | Action::CreateBranch { .. }
            | Action::Reply { .. }
            | Action::Snooze { .. }
            | Action::Unsnooze { .. }
            | Action::WakeSnoozed
            | Action::SetRule { .. }
            | Action::EditTicket { .. }
            | Action::AssignTicket { .. }
            | Action::SetPriority { .. }
            | Action::LinkPr { .. }
            | Action::UnlinkPr { .. }
            | Action::AutoLinkPrs
            | Action::RequestReview { .. }
            | Action::SyncTicketStatus
            | Action::SetStatusFollowsPr { .. }
            | Action::SelectJiraProject { .. } => self.perform_inbox(action, at_ms),
            Action::Decide { .. }
            | Action::EditSuggestion { .. }
            | Action::AddLineComment { .. }
            | Action::EditLineComment { .. }
            | Action::DeleteLineComment { .. }
            | Action::SetVerdict { .. }
            | Action::RequestAgentReview { .. }
            | Action::SendReview { .. }
            | Action::RestartCheck { .. }
            | Action::AdvanceChecks
            | Action::MergePr { .. } => self.perform_review(action),
            Action::StartAgent { .. }
            | Action::CancelAgent { .. }
            | Action::ApprovePlan { .. }
            | Action::RejectPlan { .. }
            | Action::RevisePlan { .. }
            | Action::AnswerAgent { .. }
            | Action::SteerAgent { .. }
            | Action::PauseAgent { .. }
            | Action::ResumeAgent { .. }
            | Action::AdvanceAgents
            | Action::ReportAgent { .. }
            | Action::AddressReview { .. }
            | Action::InvestigateCheck { .. }
            | Action::ApplyFix { .. }
            | Action::DiscardFix { .. }
            | Action::AddressReviewThreads { .. }
            | Action::FixFailedChecks { .. }
            | Action::CreatePullRequest { .. }
            | Action::ResumeStopped { .. } => self.perform_agent(action, at_ms),
            Action::SetNotifyChannel { .. }
            | Action::SetQuietHours { .. }
            | Action::SetAgentLimits { .. }
            | Action::SetRepoRules { .. }
            | Action::SyncRemote { .. }
            | Action::ResetDemo => self.perform_settings(action, at_ms),
        }
    }
}
