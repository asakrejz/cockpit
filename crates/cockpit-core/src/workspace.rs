//! The workspace: all data of the cockpit and the actions of the UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::ActionError;
use crate::action::LogEntry;
use crate::agent::{
    AgentOptions, AgentReport, AgentRun, Closure, FollowupTask, PullRequestRef, ReportEffect,
    RunKind,
};
use crate::attention::{Rules, Snooze};
use crate::filters::InboxFilter;
use crate::filters::Ownership;
use crate::model::*;
use crate::notify::NotifySettings;
use crate::review::{Decision, MergeBlocker, MergeMethod, Review, SendError, Verdict};
use crate::rules::RepoRules;
use crate::sync::SyncState;
use crate::text::slug;
use crate::usage::{
    AgentLimits, Charge, KEEP_DAYS, MAX_CHARGES, StopReason, Usage, day_start, month_start,
    next_month_start, sum_between,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    UnknownTicket,
    AlreadyRunning,
    /// The pull request does not exist.
    UnknownReview,
    /// The pull request is merged and stays as it is.
    Merged,
    /// The pull request has no red check.
    CheckNotFailed,
    /// The agent only continues on branches that the pattern of the repo names (`claude/…` by default).
    NotAgentBranch,
    /// The pull request has no diff the agent could look at.
    NothingToReview,
    /// As many runs are already working as the limit allows (the number).
    ParallelLimit(u32),
}

/// A pull request linked to a ticket, as the ticket detail lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkedPr {
    pub pr: PrRef,
    /// Head branch of the pull request; empty = unknown.
    pub branch: String,
}

/// Why ticket and pull request cannot be linked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LinkError {
    /// There is no review (no pull request) with this number.
    UnknownPr(PrKey),
    /// The ticket is already attached to this pull request.
    Same,
    /// The ticket already has a different pull request.
    TicketHasPr(PrKey),
    /// The pull request already belongs to a different ticket (key).
    PrHasTicket(String),
}

/// A status change that the pull request triggered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusChange {
    /// Internal ID of the ticket.
    pub item: String,
    pub key: String,
    pub pr: PrKey,
    pub from: TicketStatus,
    pub to: TicketStatus,
}

/// Why a stopped run cannot be resumed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumeError {
    /// The run is not stopped.
    NotStopped,
    ParallelLimit(u32),
    /// Even with the raise the limit is still reached.
    StillReached(StopReason),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub runs: Vec<AgentRun>,
    /// Log of all actions, oldest first. See [`crate::action`].
    #[serde(default)]
    pub log: Vec<LogEntry>,
    /// Notifications per reason and channel, plus the quiet hours.
    #[serde(default)]
    pub notifications: NotifySettings,
    /// Which attention rules apply. See [`crate::attention`].
    #[serde(default)]
    pub rules: Rules,
    /// Snoozed entries.
    #[serde(default)]
    pub snoozed: Vec<Snooze>,
    /// Is the checkmark „status follows the pull request“ off? The switch-off is stored:
    /// that way the automation applies immediately in old states.
    #[serde(default)]
    pub pr_status_off: bool,
    /// To which state of the pull request the status was last brought in line
    /// `(ticket key, status)`. A change by hand stays until
    /// the pull request moves again.
    #[serde(default)]
    pub status_seen: Vec<(String, TicketStatus)>,
    /// Prices and limits of the agents. See [`crate::usage`].
    #[serde(default)]
    pub limits: AgentLimits,
    /// Ledger: what the agents' steps cost. It stays even if a run
    /// is cancelled. „Today“ and „this month“ are derived from it.
    #[serde(default)]
    pub spend: Vec<Charge>,
    /// Rules per repository (key: name of the repo). See [`crate::rules`].
    #[serde(default)]
    pub repo_rules: BTreeMap<String, RepoRules>,
    /// Sync with Jira and GitHub: state per source, where the entries come from.
    #[serde(default)]
    pub sync: SyncState,
    /// The chosen Jira project (key, e.g. `BAU`). The inbox then shows only its
    /// tickets, and the sync fetches all open tickets of the project. `None` = all.
    #[serde(default)]
    pub jira_project: Option<String>,
    /// „Mein Team“: the Jira teams whose tickets the lists show (see [`crate::team_filter`]);
    /// change by `Action::SetTeamChoice`. No team chosen = everything.
    #[serde(
        default,
        skip_serializing_if = "crate::team_filter::TeamChoice::is_default"
    )]
    pub team_choice: crate::team_filter::TeamChoice,
    /// The chosen inbox filters (see [`crate::filters`]); change by `Action::SetInboxFilter`.
    #[serde(default)]
    pub inbox_filter: InboxFilter,
    /// The chosen filter of the Reviews tab (see [`crate::review_filter`]); change by
    /// `Action::SetReviewFilter`.
    #[serde(
        default,
        skip_serializing_if = "crate::review_filter::ReviewFilter::is_default"
    )]
    pub review_filter: crate::review_filter::ReviewFilter,
    /// Which source the overview shows (Alle / Jira / GitHub); change by
    /// `Action::SetPeopleSource`.
    #[serde(default, skip_serializing_if = "is_default_source")]
    pub people_source: crate::people_link::PeopleSource,
    /// Jira persons linked to GitHub logins (see [`crate::people_link`]); change by
    /// `Action::LinkPeople` and `Action::UnlinkPeople`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub person_links: Vec<crate::people_link::PersonLink>,
    /// Language of the interface. `None` = not chosen yet, the browser language applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<cockpit_i18n::Lang>,
}

fn is_default_source(s: &crate::people_link::PeopleSource) -> bool {
    *s == crate::people_link::PeopleSource::All
}

impl Workspace {
    // ---------- Reading ----------

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_by_key(&self, key: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.key == key)
    }

    /// The entry a run hangs on (see [`Item::matches_run_key`]).
    pub fn item_of_run(&self, run_ticket_key: &str) -> Option<&Item> {
        self.item_by_key(run_ticket_key).or_else(|| {
            self.items
                .iter()
                .find(|i| i.matches_run_key(run_ticket_key))
        })
    }

    pub(crate) fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// The Jira projects of the tickets in the workspace, sorted and without duplicates. The chosen
    /// project is always included, even if it has no tickets (yet).
    pub fn projects(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .items
            .iter()
            .filter_map(|i| i.project())
            .map(str::to_string)
            .chain(self.jira_project.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The repos of the pull requests in the inbox, sorted and without duplicates. Not to be
    /// confused with [`Workspace::repos`], the repos with rules for the agent.
    pub fn item_repos(&self) -> Vec<String> {
        let mut out: Vec<String> = self.items.iter().filter_map(|i| i.repo.clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    /// All labels of the entries, alphabetical (case-insensitive)
    /// and without duplicates.
    pub fn labels(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for label in self.items.iter().flat_map(|i| i.labels.iter()) {
            if !label.is_empty() && !out.iter().any(|l| l.eq_ignore_ascii_case(label)) {
                out.push(label.clone());
            }
        }
        out.sort_by_key(|l| l.to_lowercase());
        out
    }

    /// The owners of the entries as a choice for the filter: you first, then the others in
    /// inbox order. The agent and empty names are missing.
    pub fn owners(&self) -> Vec<Person> {
        let mut out: Vec<Person> = Vec::new();
        for owner in self.items.iter().map(|i| &i.owner) {
            let named = owner.me || owner.nobody || !owner.name.is_empty();
            if !owner.is_agent() && named && !out.contains(owner) {
                out.push(owner.clone());
            }
        }
        if let Some(i) = out.iter().position(|p| *p == Person::me()) {
            let me = out.remove(i);
            out.insert(0, me);
        }
        out
    }

    /// Does the entry match the chosen project? Without a choice everything matches; pull requests are
    /// always visible, since they belong to no Jira project.
    pub fn in_project(&self, item: &Item) -> bool {
        match (&self.jira_project, item.project()) {
            (Some(p), Some(own)) => p == own,
            (Some(_), None) => !item.is_ticket(),
            (None, _) => true,
        }
    }

    /// Chooses the Jira project. `false` if nothing changes.
    pub fn set_jira_project(&mut self, project: Option<String>) -> bool {
        if self.jira_project == project {
            return false;
        }
        self.jira_project = project;
        true
    }

    /// Does the pull request of this review concern me (author, requested reviewer or assignee)?
    /// A review without an inbox entry counts as mine: nothing says it belongs to a colleague.
    pub fn review_is_mine(&self, review: &Review) -> bool {
        let key = review.key();
        self.items
            .iter()
            .find(|i| i.pr.as_ref().is_some_and(|p| p.key().same(&key)))
            .is_none_or(Item::is_mine)
    }

    /// The reviews for the page "Pull requests": with `ownership` "Mine" only those that
    /// concern me, with „Alle“ all open pull requests of the repos. `keep` stays in the list in
    /// any case (the review that is open right now).
    pub fn reviews_for(&self, ownership: Ownership, keep: Option<&PrKey>) -> Vec<&Review> {
        self.reviews
            .iter()
            .filter(|r| {
                ownership == Ownership::All
                    || self.review_is_mine(r)
                    || keep.is_some_and(|k| k.same(&r.key()))
            })
            .collect()
    }

    /// The review of a pull request. A plain number (no repo) finds the first review with
    /// that number; use a full [`PrKey`] wherever two repos can be involved.
    pub fn review(&self, key: impl Into<PrKey>) -> Option<&Review> {
        let key = key.into();
        self.reviews.iter().find(|r| key.matches(&r.repo, r.number))
    }

    fn review_mut(&mut self, key: impl Into<PrKey>) -> Option<&mut Review> {
        let key = key.into();
        self.reviews
            .iter_mut()
            .find(|r| key.matches(&r.repo, r.number))
    }

    /// Reviews that are not sent yet.
    pub fn open_reviews(&self) -> usize {
        self.reviews.iter().filter(|r| !r.sent).count()
    }

    pub fn running_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_running()).count()
    }

    /// Agents waiting for the user: for the approval of a plan, an answer or because a limit has
    /// stopped them.
    pub fn waiting_agents(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| r.is_waiting_for_you() || r.is_stopped())
            .count()
    }

    /// Agents that a limit has stopped.
    pub fn stopped_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_stopped()).count()
    }
}

mod agents;
mod followups;
mod inbox;
mod inbox_filter;
mod limits;
pub use pull_request::PullRequestDraft;
mod pull_request;
mod review;
mod snooze;
#[cfg(test)]
mod tests;
mod tickets;
