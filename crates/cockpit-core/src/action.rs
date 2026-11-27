//! Actions and log.
//!
//! Every change to the workspace is an [`Action`]. It is applied with
//! [`Workspace::apply`](crate::Workspace::apply), which returns either an
//! [`Applied`] or an [`ActionError`]. Every successful action leaves
//! a [`LogEntry`] in the workspace's log; failures leave nothing.

use serde::{Deserialize, Serialize};

use crate::agent::{AgentOptions, AgentReport, AgentRun, Closure, PullRequestRef};
use crate::attention::Rule;
use crate::model::{Person, PrKey, Priority, TicketStatus};
use crate::notify::{Channel, NotifyReason, QuietHours};
use crate::review::{Decision, MergeBlocker, MergeMethod, SendError, Verdict};
use crate::rules::{PathViolation, RepoRules, ViolationKind};
use crate::sync::{self, Remote};
use crate::usage::{AgentLimits, StopReason, format_euro};
use crate::workspace::{AgentError, ResumeError, StatusChange, Workspace};

/// This many entries the log keeps. Older ones drop out.
pub const MAX_LOG: usize = 500;

/// A change to the workspace.
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
    /// The tick: link pull requests whose branch, title or body names a ticket key.
    AutoLinkPrs,
    /// The tick: bring the status of linked tickets in line with the pull request.
    SyncTicketStatus,
    /// Switch „status follows the pull request“ on or off.
    SetStatusFollowsPr {
        enabled: bool,
    },
    /// Choose the Jira project (key like `BAU`), `None` = all projects.
    SelectJiraProject {
        project: Option<String>,
    },
    /// Choose „Mein Team“ (see [`crate::team_filter`]): the lists then show only tickets of these
    /// teams.
    SetTeamChoice {
        choice: crate::team_filter::TeamChoice,
    },
    /// Choose the source the overview shows (Alle / Jira / GitHub).
    SetPeopleSource {
        source: crate::people_link::PeopleSource,
    },
    /// Say that a Jira person and a GitHub login are the same human (see
    /// [`crate::people_link`]).
    LinkPeople {
        jira: String,
        github: String,
    },
    /// Remove the link of a Jira person.
    UnlinkPeople {
        jira: String,
    },
    /// Apply the inbox filters (see [`crate::filters`]).
    SetInboxFilter {
        filter: crate::InboxFilter,
    },
    /// Apply the filter of the Reviews tab (repos, GitHub projects, whose pull requests).
    SetReviewFilter {
        filter: crate::review_filter::ReviewFilter,
    },
    // GitHub Projects (tickets that are issues or drafts of a project)
    /// Adds a label to the issue (`on`) or removes it.
    SetLabel {
        item: String,
        label: String,
        on: bool,
    },
    /// Closes the issue or opens it again.
    SetIssueClosed {
        item: String,
        closed: bool,
    },
    /// Sets a field of the project (`value`), or clears it (`None`). The status has its own
    /// action ([`Action::SetStatus`]).
    SetProjectField {
        item: String,
        field_id: String,
        #[serde(default)]
        value: Option<crate::FieldInput>,
    },
    /// Creates an issue in `repo` and adds it to the project `project` (its node ID), with a status
    /// if given. `created` is filled in by the backend after GitHub has answered; a value sent by
    /// the browser is dropped.
    CreateIssue {
        project: String,
        repo: String,
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        status: Option<TicketStatus>,
        #[serde(default)]
        created: Option<crate::CreatedIssue>,
    },
    /// Turns a draft issue of the project into an issue of `repo`. `created` is filled in by the
    /// backend, as for [`Action::CreateIssue`].
    ConvertDraft {
        item: String,
        repo: String,
        #[serde(default)]
        created: Option<crate::CreatedIssue>,
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
    SendReview {
        review: PrKey,
        comment: String,
    },
    /// Restarts a failed check (`check` is its name).
    RestartCheck {
        review: PrKey,
        check: String,
    },
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
    /// Report of the real agent from the container (`run` is the key of the run). Only the
    /// backend triggers it, as `Actor::Agent`; others are rejected.
    ReportAgent {
        run: String,
        report: AgentReport,
    },
    /// The pull request of the run is merged or closed (seen by the backend): the run is
    /// closed. Only the system triggers it.
    CloseAgentRun {
        run: String,
        closure: Closure,
    },
    /// Removes one run in a final state (done, cancelled, stopped, finished) from the local list.
    /// Never touches branches or pull requests, and the consumption stays booked.
    RemoveAgentRun {
        run: String,
    },
    /// Removes every run in a final state from the local list (see `RemoveAgentRun`).
    RemoveFinishedRuns,
    /// The agent reads the pull request and prepares review comments as drafts. Nothing goes
    /// to GitHub; the drafts wait for your decision.
    StartPrReview {
        review: PrKey,
        /// The language picked in the review panel; `None` = detect it.
        #[serde(default)]
        language: Option<cockpit_i18n::Lang>,
        /// The repo's `pr_language`. The backend sets it from the configuration; whatever a
        /// request carries is replaced.
        #[serde(default)]
        repo_language: Option<cockpit_i18n::Lang>,
    },
    /// Follow-up job: implement the review comments (threads) of the pull request. The backend
    /// reads them from GitHub; the agent continues on the same `claude/…` branch. The run is then
    /// called `threads-<number>`.
    AddressReviewThreads {
        review: PrKey,
        options: AgentOptions,
    },
    /// Follow-up job: fix the red checks of the pull request (names and a
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
        /// The repo the pull request is for (a run can work in several). Empty = the primary one.
        #[serde(default)]
        repo: String,
    },
    /// Resumes a stopped run after the limit that stopped it has been raised by `extra`:
    /// cents for budgets, minutes for the time limit. With `0` it continues if the
    /// limit is no longer reached at all by now (e.g. after changing the setting).
    ResumeStopped {
        run: String,
        extra: u64,
    },
    /// Choose the language of the interface (German or English).
    SetLanguage {
        language: cockpit_i18n::Lang,
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
    /// The sync: merges the state of Jira and GitHub with the workspace
    /// (see [`crate::sync`]). Only the system triggers it – the backend after every fetch.
    SyncRemote {
        remote: Remote,
    },
}

impl Action {
    /// The one actor that may apply this action; `None` for a local change that anyone may
    /// trigger (the user in the browser, the ticks of the server).
    ///
    /// * `Some(Actor::User)`: everything that reaches Jira or GitHub (every action that
    ///   `cockpit_integrations::outbound::plan_call` can turn into a call, and the pull request and
    ///   project flows of the server), merging, the follow-up jobs of the real agent and removing
    ///   runs. Neither the agent nor the system may do those.
    /// * `Some(Actor::System)`: only the backend itself (the sync, closing a finished run).
    /// * `Some(Actor::Agent)`: only the agent in the container, through the backend.
    ///
    /// The `match` has no wildcard arm on purpose: a new variant does not compile until it is
    /// classified here. [`Workspace::apply`] enforces the result before anything else happens.
    pub fn required_actor(&self) -> Option<Actor> {
        match self {
            Action::SetStatus { .. }
            | Action::CreateBranch { .. }
            | Action::Reply { .. }
            | Action::EditTicket { .. }
            | Action::AssignTicket { .. }
            | Action::SetPriority { .. }
            | Action::SetLabel { .. }
            | Action::SetIssueClosed { .. }
            | Action::SetProjectField { .. }
            | Action::CreateIssue { .. }
            | Action::ConvertDraft { .. }
            | Action::SendReview { .. }
            | Action::RestartCheck { .. }
            | Action::MergePr { .. }
            | Action::RemoveAgentRun { .. }
            | Action::RemoveFinishedRuns
            | Action::AddressReviewThreads { .. }
            | Action::FixFailedChecks { .. }
            | Action::StartPrReview { .. }
            | Action::CreatePullRequest { .. } => Some(Actor::User),
            Action::SyncRemote { .. } | Action::CloseAgentRun { .. } => Some(Actor::System),
            Action::ReportAgent { .. } => Some(Actor::Agent),
            Action::MarkDone { .. }
            | Action::Snooze { .. }
            | Action::Unsnooze { .. }
            | Action::WakeSnoozed
            | Action::SetRule { .. }
            | Action::AutoLinkPrs
            | Action::SyncTicketStatus
            | Action::SetStatusFollowsPr { .. }
            | Action::SelectJiraProject { .. }
            | Action::SetTeamChoice { .. }
            | Action::SetPeopleSource { .. }
            | Action::LinkPeople { .. }
            | Action::UnlinkPeople { .. }
            | Action::SetInboxFilter { .. }
            | Action::SetReviewFilter { .. }
            | Action::Decide { .. }
            | Action::EditSuggestion { .. }
            | Action::AddLineComment { .. }
            | Action::EditLineComment { .. }
            | Action::DeleteLineComment { .. }
            | Action::SetVerdict { .. }
            | Action::StartAgent { .. }
            | Action::CancelAgent { .. }
            | Action::ApprovePlan { .. }
            | Action::RejectPlan { .. }
            | Action::RevisePlan { .. }
            | Action::AnswerAgent { .. }
            | Action::SteerAgent { .. }
            | Action::PauseAgent { .. }
            | Action::ResumeAgent { .. }
            | Action::ResumeStopped { .. }
            | Action::SetLanguage { .. }
            | Action::SetAgentLimits { .. }
            | Action::SetRepoRules { .. }
            | Action::SetNotifyChannel { .. }
            | Action::SetQuietHours { .. } => None,
        }
    }
}

mod agent;
mod dispatch;
mod error;
mod inbox;
pub(crate) mod labels;
mod note;
mod projects;
mod review;
mod settings;
#[cfg(test)]
mod tests;

pub use error::ActionError;
use error::agent_error;
use note::{Note, status_notes};

mod log;
pub use log::*;
