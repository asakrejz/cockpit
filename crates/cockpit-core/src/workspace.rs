//! The workspace: all data of the cockpit and the actions of the UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::LogEntry;
use crate::agent::{AgentOptions, AgentReport, AgentRun, ReportEffect, RunKind, Tick};
use crate::attention::{self, Hit, Rule, Rules, Snooze};
use crate::demo;
use crate::diagnosis::diagnose;
use crate::model::*;
use crate::notify::NotifySettings;
use crate::review::{CHECK_STEPS, Decision, MergeBlocker, MergeMethod, Review, SendError, Verdict};
use crate::rules::{DEMO_REPO, RepoRules};
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
    /// No open comments are selected (or one is unknown or done).
    NoComments,
    /// Only a red check can be investigated.
    CheckNotFailed,
    /// As many runs are already working as the limit allows (the number).
    ParallelLimit(u32),
}

/// Short ID for a commit of the demo: stable, but not real.
fn short_sha(seed: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:07x}", hash & 0x0fff_ffff)
}

/// Why ticket and pull request cannot be linked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// There is no review (no pull request) with this number.
    UnknownPr(u32),
    /// The ticket is already attached to this pull request.
    Same,
    /// The ticket already has a different pull request (number).
    TicketHasPr(u32),
    /// The pull request already belongs to a different ticket (key).
    PrHasTicket(String),
}

/// A status change that the pull request triggered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusChange {
    /// Internal ID of the ticket.
    pub item: String,
    pub key: String,
    pub pr: u32,
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
    /// Links resolved by hand `(pull request, ticket key)`: the automation
    /// does not set them again.
    #[serde(default)]
    pub unlinked: Vec<(u32, String)>,
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
}

impl Workspace {
    // ---------- Reading ----------

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_by_key(&self, key: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.key == key)
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
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
            if !owner.is_agent() && !owner.name.is_empty() && !out.contains(owner) {
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

    /// Inbox entries for a filter, in display order.
    pub fn visible(&self, filter: Filter) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| filter.matches(i.scope))
            .collect()
    }

    /// Inbox entries by owner, source and search text.
    ///
    /// The rules count: an entry is only in if at least one
    /// switched-on rule applies and it is not snoozed. The filter
    /// „Zurückgestellt“ shows the snoozed entries instead.
    pub fn search(
        &self,
        filter: Filter,
        source: SourceFilter,
        query: &str,
        now_ms: u64,
    ) -> Vec<&Item> {
        self.attention(filter, now_ms)
            .into_iter()
            .filter(|i| source.matches(i.source) && crate::search::matches_query(i, query))
            .filter(|i| self.in_project(i))
            .collect()
    }

    /// Entries that need attention for this filter (or, for
    /// [`Filter::Snoozed`], are currently snoozed), in display order.
    pub fn attention(&self, filter: Filter, now_ms: u64) -> Vec<&Item> {
        self.visible(filter)
            .into_iter()
            .filter(|i| match filter {
                Filter::Snoozed => self.is_snoozed(&i.id, now_ms),
                _ => !self.is_snoozed(&i.id, now_ms) && self.needs_attention(i, now_ms),
            })
            .collect()
    }

    /// Hits of the switched-on rules for an entry, most important first.
    pub fn hits(&self, item: &Item, now_ms: u64) -> Vec<Hit> {
        attention::hits(item, &self.rules, now_ms)
    }

    pub fn needs_attention(&self, item: &Item, now_ms: u64) -> bool {
        !self.hits(item, now_ms).is_empty()
    }

    /// Entries that still need attention. For [`Filter::Snoozed`]: how many are snoozed.
    pub fn unread(&self, filter: Filter, now_ms: u64) -> usize {
        self.attention(filter, now_ms)
            .iter()
            .filter(|i| filter == Filter::Snoozed || !i.read)
            .count()
    }

    pub fn snooze_of(&self, id: &str) -> Option<&Snooze> {
        self.snoozed.iter().find(|s| s.item == id)
    }

    /// Is the entry currently snoozed?
    pub fn is_snoozed(&self, id: &str, now_ms: u64) -> bool {
        match (self.snooze_of(id), self.item(id)) {
            (Some(snooze), Some(item)) => snooze.holds(item, now_ms),
            _ => false,
        }
    }

    /// IDs of the entries whose snooze has expired or been lifted by a change
    /// (and which `wake` therefore still has to remove).
    pub fn expired_snoozes(&self, now_ms: u64) -> Vec<String> {
        self.snoozed
            .iter()
            .filter(|s| !self.is_snoozed(&s.item, now_ms))
            .map(|s| s.item.clone())
            .collect()
    }

    /// Snoozes an entry (`until_ms`: until then; `None`: until something changes).
    /// An earlier snooze is replaced.
    pub fn snooze(&mut self, id: &str, until_ms: Option<u64>) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        let snooze = Snooze::new(item, until_ms);
        self.snoozed.retain(|s| s.item != id);
        self.snoozed.push(snooze);
        true
    }

    /// Brings an entry back. `false` if it was not snoozed.
    pub fn unsnooze(&mut self, id: &str) -> bool {
        let before = self.snoozed.len();
        self.snoozed.retain(|s| s.item != id);
        self.snoozed.len() != before
    }

    /// Removes expired snoozes. Returns the IDs of the entries that came back.
    pub fn wake(&mut self, now_ms: u64) -> Vec<String> {
        let back = self.expired_snoozes(now_ms);
        self.snoozed.retain(|s| !back.contains(&s.item));
        back
    }

    /// Switches a rule on or off. `false` if nothing changes.
    pub fn set_rule(&mut self, rule: Rule, enabled: bool) -> bool {
        self.rules.set(rule, enabled)
    }

    pub fn review(&self, number: u32) -> Option<&Review> {
        self.reviews.iter().find(|r| r.number == number)
    }

    fn review_mut(&mut self, number: u32) -> Option<&mut Review> {
        self.reviews.iter_mut().find(|r| r.number == number)
    }

    /// Reviews that are not sent yet.
    pub fn open_reviews(&self) -> usize {
        self.reviews.iter().filter(|r| !r.sent).count()
    }

    pub fn running_agents(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| r.is_running() && !r.real)
            .count()
    }

    /// Agents waiting for the user: for the approval of a plan, an answer, the
    /// decision about a fix or because a limit has stopped them.
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

    // ---------- Consumption and limits ----------

    /// Consumption on the UTC day of `now_ms`, across all runs.
    pub fn spent_today(&self, now_ms: u64) -> Usage {
        let from = day_start(now_ms);
        sum_between(&self.spend, from, from + 86_400_000)
    }

    /// Consumption in the UTC month of `now_ms`, across all runs.
    pub fn spent_month(&self, now_ms: u64) -> Usage {
        sum_between(&self.spend, month_start(now_ms), next_month_start(now_ms))
    }

    /// Runs that are currently working or waiting for an answer in the middle of a step.
    pub fn consuming_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_consuming()).count()
    }

    /// Is the limit of parallel runs reached? Then the number, otherwise `None`.
    pub fn parallel_full(&self) -> Option<u32> {
        let max = self.limits.max_parallel;
        (self.consuming_agents() >= max as usize).then_some(max)
    }

    /// Sets prices and limits. `false` if they are invalid or nothing changes.
    pub fn set_limits(&mut self, limits: AgentLimits) -> bool {
        if !limits.is_valid() || limits == self.limits {
            return false;
        }
        self.limits = limits;
        true
    }

    fn charge(&mut self, at_ms: u64, ticket: &str, usage: Usage) {
        self.spend.push(Charge {
            at_ms,
            ticket: ticket.to_string(),
            usage,
        });
        let oldest = at_ms.saturating_sub(KEEP_DAYS * 86_400_000);
        self.spend.retain(|c| c.at_ms >= oldest);
        if self.spend.len() > MAX_CHARGES {
            let excess = self.spend.len() - MAX_CHARGES;
            self.spend.drain(..excess);
        }
    }

    /// Resumes a stopped run after the limit was raised by `extra` (cents for
    /// budgets, minutes for the time limit). The monthly budget applies to all runs and rises in the
    /// settings; budget and time limit of the run rise only for this run. If the limit is
    /// still reached afterwards or too many runs are already working, everything stays as it was.
    /// Returns which limit was raised.
    pub fn resume_stopped(
        &mut self,
        ticket_key: &str,
        extra: u64,
        now_ms: u64,
    ) -> Result<StopReason, ResumeError> {
        let idx = self
            .runs
            .iter()
            .position(|r| r.key() == ticket_key && r.is_stopped())
            .ok_or(ResumeError::NotStopped)?;
        if let Some(max) = self.parallel_full() {
            return Err(ResumeError::ParallelLimit(max));
        }
        let reason = self.runs[idx].stop_reason().unwrap_or(StopReason::Time);
        let mut limits = self.limits.clone();
        let mut run = self.runs[idx].clone();
        if reason == StopReason::MonthBudget {
            limits.month_budget_cents = limits.month_budget_cents.map(|b| b.saturating_add(extra));
        }
        run.resume_after_stop(extra);
        if let Some(still) = limits.reached(&run, self.spent_month(now_ms).cost_cents) {
            return Err(ResumeError::StillReached(still));
        }
        self.limits = limits;
        self.runs[idx] = run;
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        Ok(reason)
    }

    // ---------- Rules per repo ----------

    /// The repo in which a ticket is implemented. Today there is only the demo repo.
    pub fn repo_of(&self, _ticket_key: &str) -> &'static str {
        DEMO_REPO
    }

    /// The repos for which rules can be set.
    pub fn repos(&self) -> Vec<String> {
        let mut repos = vec![DEMO_REPO.to_string()];
        repos.extend(self.repo_rules.keys().filter(|k| *k != DEMO_REPO).cloned());
        repos
    }

    /// The rules of a repo. Without an entry there are none.
    pub fn rules_for(&self, repo: &str) -> RepoRules {
        self.repo_rules.get(repo).cloned().unwrap_or_default()
    }

    /// Sets the rules of a repo (cleaned up). `false` if the repo is unknown or
    /// nothing changes. Waiting plans get the new rules immediately, running runs
    /// keep those they started with.
    pub fn set_repo_rules(&mut self, repo: &str, rules: RepoRules) -> bool {
        if !self.repos().iter().any(|r| r == repo) {
            return false;
        }
        let rules = rules.cleaned();
        if self.rules_for(repo) == rules {
            return false;
        }
        for run in self
            .runs
            .iter_mut()
            .filter(|r| r.is_awaiting() && r.repo == repo)
        {
            run.rules = rules.clone();
        }
        self.repo_rules.insert(repo.to_string(), rules);
        true
    }

    /// Paused agents.
    pub fn paused_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_paused()).count()
    }

    /// Number the next pull request gets.
    pub fn next_pr_number(&self) -> u32 {
        let reviews = self.reviews.iter().map(|r| r.number);
        let items = self
            .items
            .iter()
            .filter_map(|i| i.pr.as_ref().map(|p| p.number));
        let runs = self.runs.iter().filter_map(|r| r.pr_number);
        reviews.chain(items).chain(runs).max().unwrap_or(0) + 1
    }

    // ---------- Tickets and pull requests ----------

    pub fn mark_done(&mut self, id: &str) {
        if let Some(item) = self.item_mut(id) {
            item.read = true;
        }
    }

    /// Changes the status of a ticket. Pull requests have none.
    pub fn set_status(&mut self, id: &str, status: TicketStatus) -> bool {
        match self.item_mut(id) {
            Some(item) if item.is_ticket() => {
                item.status = status;
                true
            }
            _ => false,
        }
    }

    /// Creates a branch for an entry without one and returns the name.
    pub fn create_branch(&mut self, id: &str) -> Option<String> {
        let item = self.item_mut(id).filter(|i| i.branch.is_none())?;
        let name = format!("feature/{}-{}", item.key, slug(&item.title, 24));
        item.branch = Some(name.clone());
        Some(name)
    }

    /// Replies in the ticket or pull request. Empty text is ignored.
    pub fn reply(&mut self, id: &str, text: &str) -> bool {
        let text = text.trim();
        let Some(item) = self.item_mut(id).filter(|_| !text.is_empty()) else {
            return false;
        };
        item.comments.push(Comment {
            own: true,
            ..Comment::new(Person::me(), "gerade eben", text)
        });
        true
    }

    // ---------- Edit tickets ----------

    /// Changes title and description of a ticket. The title must not be empty.
    pub fn edit_ticket(&mut self, id: &str, title: &str, description: &str) -> bool {
        let title = title.trim();
        match self.item_mut(id) {
            Some(item) if item.is_ticket() && !title.is_empty() => {
                item.title = title.to_string();
                item.description = description.trim().to_string();
                true
            }
            _ => false,
        }
    }

    /// People a ticket can be assigned to: first you, then all others who appear in the
    /// inbox, as the author of a pull request or in the comments. The agent gets tickets through
    /// its run, not through the assignment.
    pub fn people(&self) -> Vec<Person> {
        let mut list = vec![Person::me()];
        let owners = self.items.iter().map(|i| &i.owner);
        let authors = self.reviews.iter().map(|r| &r.author);
        let commenters = self
            .items
            .iter()
            .flat_map(|i| i.comments.iter().map(|c| &c.author));
        for person in owners.chain(authors).chain(commenters) {
            if !person.is_agent() && !person.name.is_empty() && !list.contains(person) {
                list.push(person.clone());
            }
        }
        list
    }

    /// Assigns a ticket to someone. Who owns it also decides whether it appears under „Ich“ or „Team“. A ticket
    /// that newly lands with you counts as unread again and triggers the rule
    /// „Zugewiesen“; if it goes to someone else, it ends.
    pub fn assign(&mut self, id: &str, person: &Person) -> bool {
        let me = Person::me();
        let Some(item) = self.item_mut(id).filter(|i| i.is_ticket()) else {
            return false;
        };
        item.owner = person.clone();
        item.scope = if *person == me {
            Scope::Mine
        } else {
            Scope::Team
        };
        if *person == me {
            item.read = false;
            item.reason = ReasonKind::Assigned;
            item.reason_text = "Dir zugewiesen".into();
            item.time = "gerade eben".into();
        }
        true
    }

    pub fn set_priority(&mut self, id: &str, priority: Priority) -> bool {
        match self.item_mut(id) {
            Some(item) if item.is_ticket() => {
                item.priority = priority;
                true
            }
            _ => false,
        }
    }

    // ---------- Link ticket and pull request ----------

    /// The ticket that belongs to this pull request (via the review).
    pub fn ticket_of_pr(&self, number: u32) -> Option<&Item> {
        let key = self.review(number)?.ticket_key.as_deref()?;
        self.item_by_key(key).filter(|i| i.is_ticket())
    }

    /// The copy of the pull request for a ticket: the entry from the inbox, otherwise from the review.
    fn pr_ref_of(&self, number: u32) -> Option<PrRef> {
        let from_item = self
            .items
            .iter()
            .filter(|i| i.kind == Kind::PullRequest)
            .find_map(|i| i.pr.clone().filter(|p| p.number == number));
        from_item.or_else(|| {
            self.review(number).map(|r| PrRef {
                number,
                draft: false,
                checks: r.checks,
                changes_requested: false,
                merged: r.merged.is_some(),
                review_requested: false,
            })
        })
    }

    /// Links a ticket with a pull request. The ticket gets the pull request (and,
    /// if it has none yet, the branch), the review gets the ticket.
    pub fn link_pr(&mut self, ticket_id: &str, number: u32) -> Result<(), LinkError> {
        let key = self
            .item(ticket_id)
            .map(|i| i.key.clone())
            .unwrap_or_default();
        let review = self.review(number).ok_or(LinkError::UnknownPr(number))?;
        let branch = review.branch.clone();
        if let Some(other) = self.ticket_of_pr(number).map(|t| t.key.clone()) {
            return Err(if other == key {
                LinkError::Same
            } else {
                LinkError::PrHasTicket(other)
            });
        }
        let current = self
            .item(ticket_id)
            .and_then(|i| i.pr.as_ref().map(|p| p.number));
        if let Some(n) = current {
            // If the ticket already points to this pull request, only the other side is missing.
            if n != number
                && self
                    .review(n)
                    .is_some_and(|r| r.ticket_key.as_deref() == Some(&key))
            {
                return Err(LinkError::TicketHasPr(n));
            }
        }
        let pr = self.pr_ref_of(number);
        if let Some(item) = self.item_mut(ticket_id) {
            item.pr = pr;
            if item.branch.is_none() && !branch.is_empty() {
                item.branch = Some(branch);
            }
        }
        if let Some(review) = self.review_mut(number) {
            review.ticket_key = Some(key.clone());
        }
        self.unlinked.retain(|(n, k)| !(*n == number && *k == key));
        Ok(())
    }

    /// Removes the link of a ticket. Returns the number of the pull request.
    /// The automation does not link the same pair again.
    pub fn unlink_pr(&mut self, ticket_id: &str) -> Option<u32> {
        let item = self.item(ticket_id).filter(|i| i.is_ticket())?;
        let (key, number) = (item.key.clone(), item.pr.as_ref()?.number);
        let review_branch = self.review(number).map(|r| r.branch.clone());
        if let Some(review) = self
            .review_mut(number)
            .filter(|r| r.ticket_key.as_deref() == Some(&key))
        {
            review.ticket_key = None;
        }
        if let Some(item) = self.item_mut(ticket_id) {
            item.pr = None;
            if item.branch == review_branch {
                item.branch = None;
            }
        }
        self.status_seen.retain(|(k, _)| *k != key);
        if !self.unlinked.contains(&(number, key.clone())) {
            self.unlinked.push((number, key));
        }
        Some(number)
    }

    /// Which pull requests the automation would link now: `(ticket ID, pull request)`.
    /// A pull request qualifies as long as no existing ticket belongs to it and
    /// branch or title name the key of a ticket without a pull request. Pairs
    /// resolved by hand stay resolved.
    pub fn auto_links(&self) -> Vec<(String, u32)> {
        let mut taken: Vec<&str> = Vec::new();
        let mut links = Vec::new();
        for review in self
            .reviews
            .iter()
            .filter(|r| self.ticket_of_pr(r.number).is_none())
        {
            let free = self.items.iter().filter(|i| {
                i.is_ticket()
                    && i.pr.is_none()
                    && !taken.contains(&i.key.as_str())
                    && !self.unlinked.contains(&(review.number, i.key.clone()))
            });
            let known = free.clone().map(|i| i.key.as_str());
            if let Some(key) = crate::link::find_ticket(&[&review.branch, &review.title], known)
                && let Some(ticket) = free.clone().find(|i| i.key == key)
            {
                taken.push(&ticket.key);
                links.push((ticket.id.clone(), review.number));
            }
        }
        links
    }

    /// Links what `auto_links` finds. Returns `(ticket ID, key, pull request)`.
    pub fn auto_link(&mut self) -> Vec<(String, String, u32)> {
        let mut done = Vec::new();
        for (id, number) in self.auto_links() {
            if self.link_pr(&id, number).is_ok() {
                let key = self.item(&id).map(|i| i.key.clone()).unwrap_or_default();
                done.push((id, key, number));
            }
        }
        done
    }

    // ---------- Status follows the pull request ----------

    /// Linked tickets with the status their pull request suggests: `(index, pull request, target)`.
    fn linked_targets(&self) -> Vec<(usize, u32, TicketStatus)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.is_ticket())
            .filter_map(|(at, i)| {
                let pr = i.pr.as_ref()?;
                let is_linked = self
                    .review(pr.number)
                    .is_some_and(|r| r.ticket_key.as_deref() == Some(i.key.as_str()));
                is_linked.then(|| (at, pr.number, pr.target_status()))
            })
            .collect()
    }

    /// Has a pull request moved since the last bring-in-line? (Without changing anything.)
    pub fn status_pending(&self) -> bool {
        !self.pr_status_off
            && self.linked_targets().into_iter().any(|(at, _, target)| {
                let key = &self.items[at].key;
                self.status_seen
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, t)| *t)
                    != Some(target)
            })
    }

    /// Notes that the status matches the current state of the pull request, without changing it.
    fn mark_status_seen(&mut self, key: &str, target: TicketStatus) {
        match self.status_seen.iter_mut().find(|(k, _)| k == key) {
            Some((_, seen)) => *seen = target,
            None => self.status_seen.push((key.to_string(), target)),
        }
    }

    /// Brings the status of linked tickets in line: pull request merged → Done, review
    /// requested → Review, otherwise → In progress. Only if the pull request's state has changed since
    /// the last time; a status you set by hand afterwards stays
    /// until the pull request moves again. With the automation switched off it does nothing.
    pub fn follow_status(&mut self) -> Vec<StatusChange> {
        if self.pr_status_off {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for (at, pr, target) in self.linked_targets() {
            let key = self.items[at].key.clone();
            let seen = self.status_seen.iter().find(|(k, _)| *k == key);
            if seen.is_some_and(|(_, t)| *t == target) {
                continue;
            }
            self.mark_status_seen(&key, target);
            let item = &mut self.items[at];
            if item.status == target {
                continue;
            }
            changes.push(StatusChange {
                item: item.id.clone(),
                key,
                pr,
                from: item.status,
                to: target,
            });
            item.status = target;
            if target == TicketStatus::Done {
                item.read = true;
                item.agent_note = None;
            }
        }
        changes
    }

    /// Switches „status follows the pull request“ on or off. `false` if nothing changes.
    pub fn set_status_follows_pr(&mut self, enabled: bool) -> bool {
        if self.pr_status_off != enabled {
            return false;
        }
        self.pr_status_off = !enabled;
        true
    }

    // ---------- Review ----------

    pub fn decide(&mut self, number: u32, suggestion: &str, decision: Option<Decision>) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.decide(suggestion, decision))
    }

    pub fn edit_suggestion(&mut self, number: u32, suggestion: &str, text: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit(suggestion, text))
    }

    /// Own comment on a diff line. Returns the ID of the comment.
    pub fn add_line_comment(
        &mut self,
        number: u32,
        file: &str,
        line: usize,
        text: &str,
        suggestion: Option<&str>,
    ) -> Option<String> {
        self.review_mut(number)?
            .add_line_comment(file, line, text, suggestion)
    }

    pub fn edit_line_comment(
        &mut self,
        number: u32,
        comment: &str,
        text: &str,
        suggestion: Option<&str>,
    ) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit_line_comment(comment, text, suggestion))
    }

    pub fn delete_line_comment(&mut self, number: u32, comment: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.delete_line_comment(comment))
    }

    pub fn set_verdict(&mut self, number: u32, verdict: Verdict) {
        if let Some(r) = self.review_mut(number) {
            r.set_verdict(verdict);
        }
    }

    /// Lets the agent look through a pull request. Returns the number of suggestions.
    pub fn request_agent_review(&mut self, number: u32) -> usize {
        let count = self.review_mut(number).map_or(0, Review::run_agent_review);
        if let Some(item) = self.pr_item_mut(number) {
            item.agent_note = Some(match count {
                0 => "Der Agent hat nichts zu beanstanden".to_string(),
                1 => "1 Kommentar vom Agenten wartet auf deine Freigabe".to_string(),
                n => format!("{n} Kommentare vom Agenten warten auf deine Freigabe"),
            });
        }
        count
    }

    /// The entry that stands for this pull request in the inbox.
    fn pr_item_mut(&mut self, number: u32) -> Option<&mut Item> {
        self.items
            .iter_mut()
            .find(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
    }

    /// Sends the review. The inbox entry gets a summary as
    /// an own comment and counts as done. Returns the number of comments.
    pub fn send_review(&mut self, number: u32, own_comment: &str) -> Result<usize, SendError> {
        let review = self.review_mut(number).ok_or(SendError::UnknownReview)?;
        let verdict = review.verdict;
        let sent = review.send()?.len();

        let mut text = format!(
            "Review gesendet: {} · {sent} {}",
            verdict.label(),
            if sent == 1 { "Kommentar" } else { "Kommentare" }
        );
        if !own_comment.trim().is_empty() {
            text.push('\n');
            text.push_str(own_comment.trim());
        }
        if let Some(item) = self.pr_item_mut(number) {
            item.comments.push(Comment {
                own: true,
                ..Comment::new(Person::me(), "gerade eben", &text)
            });
            item.agent_note = None;
            item.read = true;
            // The review is submitted; the request is answered.
            if item.reason == ReasonKind::ReviewRequested {
                item.reason = ReasonKind::Comment;
            }
        }
        Ok(sent)
    }

    // ---------- Checks ----------

    /// Checks that are still running (across all pull requests). Pull requests from GitHub do not
    /// count: their checks run there, the sync fetches the result.
    pub fn running_checks(&self) -> usize {
        self.reviews
            .iter()
            .filter(|r| self.sync.pr(r.number).is_none())
            .map(Review::running_checks)
            .sum()
    }

    /// Writes the overall state of the checks into the inbox entries.
    fn sync_pr_checks(&mut self, number: u32) {
        let Some(checks) = self.review(number).map(|r| r.checks) else {
            return;
        };
        for item in self
            .items
            .iter_mut()
            .filter(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
        {
            if let Some(pr) = item.pr.as_mut() {
                pr.checks = checks;
            }
            // Green again: the entry no longer hangs on the checks.
            if checks == Checks::Passed && item.reason == ReasonKind::ChecksFailed {
                item.reason = ReasonKind::ReviewRequested;
                item.reason_text = "Checks wieder bestanden".into();
            }
        }
    }

    /// Restarts a failed check.
    pub fn restart_check(&mut self, number: u32, check: &str) -> bool {
        let restarted = self
            .review_mut(number)
            .is_some_and(|r| r.restart_check(check));
        if restarted {
            self.sync_pr_checks(number);
        }
        restarted
    }

    /// The tick of the checks: every running one advances one step. Returns
    /// `(pull request, check, result)` for each one that finished in the process.
    pub fn advance_checks(&mut self) -> Vec<(u32, String, Checks)> {
        let mut finished = Vec::new();
        let linked: Vec<u32> = self.sync.prs.iter().map(|p| p.number).collect();
        for review in self
            .reviews
            .iter_mut()
            .filter(|r| !linked.contains(&r.number))
        {
            for name in review.advance_checks() {
                let status = review
                    .check_runs
                    .iter()
                    .find(|c| c.name == name)
                    .map_or(Checks::Failed, |c| c.status);
                finished.push((review.number, name, status));
            }
        }
        let numbers: Vec<u32> = finished.iter().map(|(n, ..)| *n).collect();
        for n in numbers {
            self.sync_pr_checks(n);
        }
        finished
    }

    // ---------- Merging ----------

    /// What prevents merging pull request `number`. Unknown = empty.
    pub fn merge_blockers(&self, number: u32) -> Vec<MergeBlocker> {
        let Some(review) = self.review(number) else {
            return Vec::new();
        };
        let mut blockers = review.merge_blockers();
        let draft = self
            .items
            .iter()
            .any(|i| i.pr.as_ref().is_some_and(|p| p.number == number && p.draft));
        if draft && review.merged.is_none() {
            blockers.insert(0, MergeBlocker::Draft);
        }
        blockers
    }

    /// Merges the pull request. The inbox entry shows „Gemergt“. The linked ticket
    /// follows via [`Workspace::follow_status`] (if that is switched on); returns its
    /// status change.
    pub fn merge_pr(
        &mut self,
        number: u32,
        method: MergeMethod,
    ) -> Result<Vec<StatusChange>, Vec<MergeBlocker>> {
        let blockers = self.merge_blockers(number);
        if !blockers.is_empty() {
            return Err(blockers);
        }
        let Some(review) = self.review_mut(number) else {
            return Err(Vec::new());
        };
        review.merged = Some(method);
        let ticket = review.ticket_key.clone();

        for item in self
            .items
            .iter_mut()
            .filter(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
        {
            if let Some(pr) = item.pr.as_mut() {
                pr.merged = true;
                pr.draft = false;
            }
            item.read = true;
            item.agent_note = None;
            if item.kind == Kind::PullRequest {
                item.reason_text = format!("Gemergt ({})", method.label());
            }
        }
        // A linked ticket without its own copy of the pull request gets it now.
        if let Some(id) = ticket
            .and_then(|key| self.item_by_key(&key))
            .filter(|i| i.is_ticket() && i.pr.is_none())
            .map(|i| i.id.clone())
        {
            let pr = self.pr_ref_of(number);
            if let (Some(item), Some(pr)) = (self.item_mut(&id), pr) {
                item.pr = Some(pr);
            }
        }
        Ok(self.follow_status())
    }

    /// Requests a review: the pull request counts as „Review angefragt“. `false` if it already is.
    pub fn request_review(&mut self, number: u32) -> bool {
        let mut changed = false;
        for pr in self
            .items
            .iter_mut()
            .filter_map(|i| i.pr.as_mut())
            .filter(|p| p.number == number && !p.review_requested)
        {
            pr.review_requested = true;
            changed = true;
        }
        changed
    }

    // ---------- Agents ----------

    /// The run for a ticket. If there is none yet, a ready-to-start one is returned.
    pub fn run(&self, ticket_key: &str) -> Option<AgentRun> {
        if let Some(run) = self.runs.iter().find(|r| r.key() == ticket_key) {
            let mut run = run.clone();
            // A ready-to-start run always shows the current rules.
            if run.is_idle() {
                self.apply_rules(&mut run);
            }
            return Some(run);
        }
        let item = self.item_by_key(ticket_key).filter(|i| i.is_ticket())?;
        let mut run = AgentRun::new(&item.key, &item.title);
        self.apply_rules(&mut run);
        run.comments = item.comments.len();
        run.files = demo::agent_changes(&item.key)
            .into_iter()
            .map(|f| f.name)
            .collect();
        Some(run)
    }

    fn apply_rules(&self, run: &mut AgentRun) {
        run.repo = self.repo_of(&run.ticket_key).to_string();
        run.rules = self.rules_for(&run.repo);
    }

    /// Commissions the agent with a ticket.
    pub fn start_agent(
        &mut self,
        ticket_key: &str,
        options: AgentOptions,
    ) -> Result<(), AgentError> {
        let mut run = self.run(ticket_key).ok_or(AgentError::UnknownTicket)?;
        // Follow-up jobs start via the review, not via a ticket.
        if run.is_followup() {
            return Err(AgentError::UnknownTicket);
        }
        if run.is_active() {
            return Err(AgentError::AlreadyRunning);
        }
        run.start(options, self.next_pr_number());
        // A run that works immediately needs a free slot.
        if run.is_consuming()
            && let Some(max) = self.parallel_full()
        {
            return Err(AgentError::ParallelLimit(max));
        }
        let waiting = run.is_awaiting();
        match self.runs.iter_mut().find(|r| r.key() == ticket_key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket_key) {
            // If the agent is still waiting for approval, the ticket stays unchanged.
            if waiting {
                item.agent_note = Some("Der Agent wartet auf die Freigabe seines Plans".into());
            } else {
                item.status = TicketStatus::InProgress;
                item.agent_note = Some("Der Agent arbeitet an der Umsetzung".into());
            }
        }
        Ok(())
    }

    fn active_run_mut(&mut self, key: &str) -> Option<&mut AgentRun> {
        self.runs
            .iter_mut()
            .find(|r| r.key() == key && r.is_active())
    }

    /// The entry the run hangs on: for follow-up jobs the one with the pull request.
    fn run_item_mut(&mut self, key: &str) -> Option<&mut Item> {
        let item_key = self
            .runs
            .iter()
            .find(|r| r.key() == key)
            .map(|r| r.ticket_key.clone())?;
        self.items.iter_mut().find(|i| i.key == item_key)
    }

    fn clear_agent_note(&mut self, key: &str) {
        if let Some(item) = self.run_item_mut(key) {
            item.agent_note = None;
        }
    }

    /// A follow-up job that no longer runs leaves no run behind. (For the ticket
    /// the ready run stays, as before.)
    fn drop_idle_followups(&mut self) {
        self.runs.retain(|r| !(r.is_followup() && r.is_idle()));
    }

    /// Cancels a waiting or running agent.
    pub fn cancel_agent(&mut self, ticket_key: &str) {
        let Some(run) = self.active_run_mut(ticket_key) else {
            return;
        };
        run.reset();
        self.clear_agent_note(ticket_key);
        self.drop_idle_followups();
    }

    /// Approves the plan: the agent gets going, the ticket goes to „In Arbeit“.
    pub fn approve_plan(&mut self, ticket_key: &str) -> bool {
        let followup = self
            .runs
            .iter()
            .any(|r| r.key() == ticket_key && r.is_followup());
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::approve)
        {
            return false;
        }
        if let Some(item) = self.run_item_mut(ticket_key) {
            // A follow-up job does not change the ticket's status.
            if !followup {
                item.status = TicketStatus::InProgress;
            }
            item.agent_note = Some("Der Agent arbeitet an der Umsetzung".into());
        }
        true
    }

    /// Rejects the plan. The run ends without traces.
    pub fn reject_plan(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::reject)
        {
            return false;
        }
        self.clear_agent_note(ticket_key);
        self.drop_idle_followups();
        true
    }

    /// Requests a new plan with a note.
    pub fn revise_plan(&mut self, ticket_key: &str, note: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.revise(note))
    }

    fn set_agent_note(&mut self, key: &str, note: &str) {
        if let Some(item) = self.run_item_mut(key) {
            item.agent_note = Some(note.into());
        }
    }

    /// Answers the agent's question; it keeps working afterwards.
    pub fn answer_agent(&mut self, ticket_key: &str, text: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(|r| r.answer(text))
        {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        true
    }

    /// Sends the running agent a hint for the next step.
    pub fn steer_agent(&mut self, ticket_key: &str, text: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.steer(text))
    }

    /// Stops a running agent.
    pub fn pause_agent(&mut self, ticket_key: &str) -> bool {
        if !self.active_run_mut(ticket_key).is_some_and(AgentRun::pause) {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent ist pausiert");
        true
    }

    /// Resumes a paused agent.
    pub fn resume_agent(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::resume)
        {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        true
    }

    /// Lets every running agent complete a step or ask a question.
    /// Returns the keys of the runs that finished in the process. Without a clock: bookings
    /// carry the time 0. Whoever knows the time takes [`Workspace::advance_agents_at`].
    pub fn advance_agents(&mut self) -> Vec<String> {
        self.advance_agents_at(0)
    }

    /// Like [`Workspace::advance_agents`], with the time of the tick (milliseconds since 1970).
    /// The consumption of each step goes into the ledger; if a limit is reached, the run stops.
    pub fn advance_agents_at(&mut self, now_ms: u64) -> Vec<String> {
        let mut finished = Vec::new();
        let mut asking = Vec::new();
        let mut diagnosed = Vec::new();
        let mut stopped = Vec::new();
        let mut charges = Vec::new();
        let limits = self.limits.clone();
        let mut month_spent = self.spent_month(now_ms).cost_cents;
        for run in &mut self.runs {
            let before = run.step_usage.len();
            let tick = run.tick_with(&limits, month_spent);
            // Newly reported consumption belongs in the ledger.
            if run.step_usage.len() > before
                && let Some(usage) = run.step_usage.last().copied()
            {
                month_spent = month_spent.saturating_add(usage.cost_cents);
                charges.push((run.ticket_key.clone(), usage));
            }
            match tick {
                Tick::Finished => finished.push(run.clone()),
                Tick::Asked => asking.push(run.key()),
                Tick::Diagnosed => diagnosed.push(run.key()),
                Tick::Stopped => stopped.push((run.key(), run.stop_reason())),
                Tick::Stepped | Tick::Idle => {}
            }
        }
        for (key, usage) in charges {
            self.charge(now_ms, &key, usage);
        }
        for (key, reason) in &stopped {
            if let Some(reason) = reason {
                self.set_agent_note(
                    key,
                    &format!("Der Agent ist angehalten: {}", reason.label()),
                );
            }
        }
        for key in &asking {
            self.set_agent_note(key, "Der Agent hat eine Frage an dich");
        }
        for key in &diagnosed {
            self.diagnose_run(key);
        }
        for run in &finished {
            if run.is_followup() {
                self.finish_followup(run);
            } else {
                self.publish(run);
            }
        }
        finished.into_iter().map(|r| r.key()).collect()
    }

    /// Books a report of the real agent (from the container) on run `key`: state,
    /// consumption in the ledger, note on the ticket. `None` if the report does not fit – then
    /// everything stays as it was.
    pub fn report_agent(
        &mut self,
        key: &str,
        report: AgentReport,
        now_ms: u64,
    ) -> Option<ReportEffect> {
        let limits = self.limits.clone();
        let month_spent = self.spent_month(now_ms).cost_cents;
        let run = self
            .runs
            .iter_mut()
            .find(|r| r.key() == key && r.is_active())?;
        let ticket = run.ticket_key.clone();
        let effect = run.report(report, &limits, month_spent)?;
        let run = run.clone();
        if let Some(usage) = effect.usage {
            self.charge(now_ms, &ticket, usage);
        }
        if effect.failed {
            self.clear_agent_note(key);
            self.drop_idle_followups();
        } else if effect.finished {
            self.finish_real(&run);
        } else if let Some(reason) = effect.stopped {
            self.set_agent_note(
                key,
                &format!("Der Agent ist angehalten: {}", reason.label()),
            );
        } else if effect.asking {
            self.set_agent_note(key, "Der Agent hat eine Frage an dich");
        } else if run.is_awaiting() {
            self.set_agent_note(
                key,
                if run.is_drafting() {
                    "Der Agent erarbeitet seinen Plan"
                } else {
                    "Der Agent wartet auf die Freigabe seines Plans"
                },
            );
        } else if run.is_running() {
            self.set_agent_note(key, "Der Agent arbeitet an der Umsetzung");
        }
        Some(effect)
    }

    /// The real agent is done: the branch is on GitHub. The agent does not create a
    /// pull request – you do that; so there is no review yet.
    fn finish_real(&mut self, run: &AgentRun) {
        let branch = run.branch();
        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(branch.clone());
            item.agent_note = Some(format!(
                "Vom Agenten umgesetzt – Branch {branch} ist gepusht, den Pull Request legst du an"
            ));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Branch {branch} gepusht");
            item.time = "gerade eben".into();
            item.read = false;
        }
    }

    // ---------- Follow-up jobs: implement review comments, investigate red checks ----------

    /// The key of the entry the pull request hangs on (ticket or PR entry).
    fn item_key_of_pr(&self, number: u32) -> String {
        self.items
            .iter()
            .find(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
            .map_or_else(|| format!("#{number}"), |i| i.key.clone())
    }

    /// Is a follow-up job already running or waiting on this pull request?
    pub fn active_followup(&self, review: u32) -> Option<&AgentRun> {
        self.runs
            .iter()
            .find(|r| r.is_followup() && r.kind.review() == Some(review) && r.is_active())
    }

    /// All follow-up jobs of a pull request, also the finished ones.
    pub fn followups(&self, review: u32) -> Vec<&AgentRun> {
        self.runs
            .iter()
            .filter(|r| r.is_followup() && r.kind.review() == Some(review))
            .collect()
    }

    /// Checks that a follow-up job may start on the pull request.
    fn check_followup(&self, review: u32) -> Result<&Review, AgentError> {
        let r = self.review(review).ok_or(AgentError::UnknownReview)?;
        if r.merged.is_some() {
            return Err(AgentError::Merged);
        }
        if self.active_followup(review).is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        Ok(r)
    }

    fn start_followup(&mut self, mut run: AgentRun, options: AgentOptions) -> String {
        run.start(options, 0);
        let key = run.key();
        let waiting = run.is_awaiting();
        let note = match &run.kind {
            _ if waiting => "Der Agent wartet auf die Freigabe seines Plans",
            RunKind::Diagnose { .. } => "Der Agent untersucht einen roten Check",
            _ => "Der Agent setzt Review-Kommentare um",
        };
        let number = run.pr_number;
        match self.runs.iter_mut().find(|r| r.key() == key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = number.and_then(|n| self.pr_item_mut(n)) {
            item.agent_note = Some(note.into());
        }
        key
    }

    /// Commissions the agent to implement comments of a review. Only open
    /// line comments of this review count. Returns the key of the run (`review-91`).
    ///
    /// The run pushes to the pull request's branch; there is no new pull request.
    pub fn address_review(
        &mut self,
        review: u32,
        comments: &[String],
        options: AgentOptions,
    ) -> Result<String, AgentError> {
        let r = self.check_followup(review)?;
        let mut files: Vec<String> = Vec::new();
        for id in comments {
            let c = r
                .line_comments
                .iter()
                .find(|c| c.id == *id && c.is_open())
                .ok_or(AgentError::NoComments)?;
            if !files.contains(&c.file) {
                files.push(c.file.clone());
            }
        }
        if comments.is_empty() {
            return Err(AgentError::NoComments);
        }
        let mut ids: Vec<String> = Vec::new();
        for id in comments {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let mut run = AgentRun::followup(
            RunKind::AddressReview {
                review,
                comments: ids,
            },
            &self.item_key_of_pr(review),
            &title,
            &branch,
        );
        run.files = files;
        Ok(self.start_followup(run, options))
    }

    /// Commissions the agent to investigate a red check. It presents a diagnosis and
    /// changes nothing before you adopt the fix. Returns the key of the run.
    pub fn investigate_check(&mut self, review: u32, check: &str) -> Result<String, AgentError> {
        let r = self.check_followup(review)?;
        let failed = r
            .check_runs
            .iter()
            .any(|c| c.name == check && c.status == Checks::Failed);
        if !failed {
            return Err(AgentError::CheckNotFailed);
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let run = AgentRun::followup(
            RunKind::Diagnose {
                review,
                check: check.to_string(),
            },
            &self.item_key_of_pr(review),
            &title,
            &branch,
        );
        // An investigation changes nothing before you adopt the fix – it needs no plan.
        let options = AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        };
        Ok(self.start_followup(run, options))
    }

    /// The investigation has read the log: the diagnosis is created and attached to the run.
    fn diagnose_run(&mut self, key: &str) {
        let Some((review, check)) =
            self.runs
                .iter()
                .find(|r| r.key() == key)
                .and_then(|r| match &r.kind {
                    RunKind::Diagnose { review, check } => Some((*review, check.clone())),
                    _ => None,
                })
        else {
            return;
        };
        let diagnosis = self.review(review).and_then(|r| {
            let c = r.check_runs.iter().find(|c| c.name == check)?;
            Some(diagnose(r, c))
        });
        let note = match &diagnosis {
            Some(d) => format!("Diagnose zu „{check}“ liegt vor: {}", d.cause),
            None => format!("Der Check „{check}“ ist nicht mehr da"),
        };
        if let Some(run) = self.runs.iter_mut().find(|r| r.key() == key) {
            run.diagnosis = diagnosis;
        }
        self.set_agent_note(key, &note);
    }

    /// Adopts the fix of the diagnosis: the agent applies it, pushes and restarts the check.
    pub fn accept_fix(&mut self, key: &str) -> bool {
        if !self.active_run_mut(key).is_some_and(AgentRun::accept_fix) {
            return false;
        }
        self.set_agent_note(key, "Der Agent wendet den Fix an");
        true
    }

    /// Discards the diagnosis: the run ends, the branch stays untouched.
    pub fn discard_fix(&mut self, key: &str) -> bool {
        if !self.runs.iter().any(|r| r.key() == key && r.is_diagnosed()) {
            return false;
        }
        if let Some(run) = self.active_run_mut(key) {
            run.reset();
        }
        self.clear_agent_note(key);
        self.drop_idle_followups();
        true
    }

    /// A follow-up job is done: the new commit is on the pull request's branch.
    fn finish_followup(&mut self, run: &AgentRun) {
        let Some(number) = run.kind.review() else {
            return;
        };
        match &run.kind {
            RunKind::AddressReview { comments, .. } => {
                let key = run.key();
                let sha = short_sha(&format!("{key}:{}", comments.join(",")));
                let done = self.review_mut(number).map_or(0, |r| {
                    // Change suggestions land in the diff, then the comments are done.
                    for id in comments {
                        r.apply_comment_suggestion(id);
                    }
                    let done = r.resolve_comments(comments, &key);
                    // The review is a new round: the old verdict no longer applies.
                    r.verdict = Verdict::Comment;
                    r.sent = false;
                    done
                });
                let what = match done {
                    1 => "1 Review-Kommentar".to_string(),
                    n => format!("{n} Review-Kommentare"),
                };
                if let Some(item) = self.pr_item_mut(number) {
                    if let Some(pr) = item.pr.as_mut() {
                        pr.changes_requested = false;
                    }
                    item.agent_note = Some(format!("Neuer Commit {sha}: {what} umgesetzt"));
                    item.reason = ReasonKind::ReviewRequested;
                    item.reason_text = format!("Review angefragt · Agent hat {what} umgesetzt");
                    item.time = "gerade eben".into();
                    item.read = false;
                    item.comments.push(Comment::new(
                        Person::agent(),
                        "gerade eben",
                        &format!(
                            "Commit {sha} auf {}: {what} umgesetzt. Bitte noch einmal prüfen.",
                            run.branch()
                        ),
                    ));
                }
            }
            RunKind::Diagnose { check, .. } => {
                let fix = run.diagnosis.as_ref().and_then(|d| d.fix.clone());
                let pushed = self
                    .review_mut(number)
                    .zip(fix.as_ref())
                    .is_some_and(|(r, fix)| {
                        let applied = r.apply_fix(fix);
                        if applied {
                            // The fix removes the error; a flaky check only needs the restart.
                            r.set_rerun(check, Checks::Passed);
                        }
                        applied
                    });
                self.restart_check(number, check);
                let sha = short_sha(&format!("{}:{check}", run.key()));
                let note = if pushed {
                    format!("Fix {sha} gepusht, „{check}“ läuft erneut")
                } else {
                    format!("„{check}“ neu gestartet")
                };
                if let Some(item) = self.pr_item_mut(number) {
                    item.agent_note = Some(note.clone());
                    if pushed {
                        item.comments.push(Comment::new(
                            Person::agent(),
                            "gerade eben",
                            &format!("Commit {sha} auf {}: {note}.", run.branch()),
                        ));
                    }
                }
            }
            RunKind::Implement => {}
        }
    }

    /// A run is done: branch and pull request appear on the ticket, and the
    /// pull request waits as a review.
    fn publish(&mut self, run: &AgentRun) {
        let Some(number) = run.pr_number else { return };
        let checks = if run.options.run_tests {
            Checks::Passed
        } else {
            Checks::Running
        };
        let tests = if run.options.run_tests {
            "Tests bestanden"
        } else {
            "Tests übersprungen"
        };

        let mut seen = None;
        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(run.branch());
            item.pr = Some(PrRef {
                number,
                draft: run.options.draft_pr,
                checks,
                changes_requested: false,
                merged: false,
                review_requested: true,
            });
            // The pull request waits as a review – the same way as for any linked ticket.
            item.status = item
                .pr
                .as_ref()
                .map_or(TicketStatus::Review, PrRef::target_status);
            seen = Some((item.key.clone(), item.status));
            item.agent_note = Some(format!("Vom Agenten umgesetzt · {tests}"));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Pull Request #{number} erstellt");
            item.time = "gerade eben".into();
            item.read = false;
        }
        if let Some((key, status)) = seen {
            self.mark_status_seen(&key, status);
        }
        self.reviews.push(Review {
            number,
            title: run.ticket_title.clone(),
            description: format!(
                "Vom Agenten für {} umgesetzt · {tests}. Der Agent führt keinen Merge aus.",
                run.ticket_key
            ),
            author: Person::agent(),
            branch: run.branch(),
            base: run.options.base_branch.clone(),
            ticket_key: Some(run.ticket_key.clone()),
            checks,
            files: demo::agent_changes(&run.ticket_key),
            agent_reviewed: false,
            suggestions: Vec::new(),
            line_comments: Vec::new(),
            verdict: Verdict::Comment,
            sent: false,
            check_runs: {
                let mut runs = demo::passing_checks();
                // Without tests the test check is still running; the tick of the checks completes it.
                if checks == Checks::Running
                    && let Some(tests) = runs.last_mut()
                {
                    tests.status = Checks::Running;
                    tests.remaining = CHECK_STEPS;
                    tests.log = "$ cargo test\nwird ausgeführt …\n".into();
                }
                runs
            },
            merged: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::STEPS;
    use crate::attention::Rule;

    fn ws() -> Workspace {
        demo::workspace()
    }

    /// Options without plan approval: the agent runs immediately.
    fn direct() -> AgentOptions {
        AgentOptions {
            plan_first: false,
            ..Default::default()
        }
    }

    /// Lets all agents run to the end and answers every follow-up question.
    fn finish_agents(w: &mut Workspace) -> Vec<String> {
        let mut finished = Vec::new();
        for _ in 0..=STEPS {
            let asking: Vec<String> = w
                .runs
                .iter()
                .filter(|r| r.is_asking())
                .map(|r| r.ticket_key.clone())
                .collect();
            for key in asking {
                assert!(w.answer_agent(&key, "Entfernen"));
            }
            finished.extend(w.advance_agents());
        }
        finished
    }

    #[test]
    fn demo_data_matches_the_design() {
        let w = ws();
        assert_eq!(w.items.len(), 7);
        assert_eq!(w.unread(Filter::All, NOW), 6, "pr85 is not mine");
        assert_eq!(
            (w.visible(Filter::Mine).len(), w.visible(Filter::Team).len()),
            (5, 2)
        );
        assert_eq!(w.review(87).unwrap().suggestions.len(), 3);
        assert_eq!(w.open_reviews(), 3);
        assert_eq!(w.running_agents(), 0);
        // Every pull request on an entry has a review.
        // (Exception: #12 in the second repo, which only shows the inbox filters.)
        for pr in w
            .items
            .iter()
            .filter_map(|i| i.pr.as_ref())
            .filter(|p| p.number != 12)
        {
            assert!(w.review(pr.number).is_some(), "#{}", pr.number);
        }
    }

    #[test]
    fn done_items_stop_counting() {
        let mut w = ws();
        assert_eq!(w.unread(Filter::Mine, NOW), 5);
        w.mark_done("bau142");
        assert_eq!(w.unread(Filter::All, NOW), 5);
        assert_eq!(w.unread(Filter::Mine, NOW), 4);
        assert_eq!(w.unread(Filter::Team, NOW), 1);
    }

    /// 11.11.2026, 12:00 UTC.
    const NOW: u64 = 1_794_398_400_000;
    const HOUR: u64 = 3_600_000;

    fn ids(items: Vec<&Item>) -> Vec<&str> {
        items.into_iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn the_inbox_follows_the_rules() {
        let mut w = ws();
        // pr85 (checks red, but from Mira) matches no rule and is missing.
        assert_eq!(
            ids(w.attention(Filter::All, NOW)),
            ["bau142", "pr87", "bau138", "bau150", "doc7", "pr12"]
        );
        assert_eq!(ids(w.attention(Filter::Team, NOW)), ["pr87"]);
        // Rule off: the hits behind it disappear – bau150 stays because of „Zugewiesen“.
        w.set_rule(Rule::ReviewRequested, false);
        assert_eq!(ids(w.attention(Filter::Team, NOW)), Vec::<&str>::new());
        w.set_rule(Rule::Overdue, false);
        w.set_rule(Rule::Assigned, false);
        assert!(!ids(w.attention(Filter::All, NOW)).contains(&"bau150"));
        // The reason of the entry is the first hit.
        assert_eq!(
            w.hits(w.item("bau142").unwrap(), NOW)[0].rule,
            Rule::Mention
        );
        // Answering resolves the mention.
        w.reply("bau142", "Mache ich.");
        w.set_rule(Rule::Assigned, true);
        assert_eq!(
            w.hits(w.item("bau142").unwrap(), NOW)[0].rule,
            Rule::Assigned
        );
    }

    #[test]
    fn counts_follow_the_rules_and_snoozing() {
        let mut w = ws();
        assert_eq!(w.unread(Filter::Mine, NOW), 5);
        w.set_rule(Rule::Mention, false);
        w.set_rule(Rule::Assigned, false);
        // bau142 and doc7 now have no reason left; bau138 (blocked), bau150 (overdue)
        // and pr12 (blocked) stay.
        assert_eq!(w.unread(Filter::Mine, NOW), 3);
        assert!(w.snooze("bau138", Some(NOW + HOUR)));
        assert_eq!(w.unread(Filter::Mine, NOW), 2);
        assert_eq!(w.unread(Filter::Snoozed, NOW), 1);
        // After expiry it counts again – even if nobody calls `wake`.
        assert_eq!(w.unread(Filter::Mine, NOW + HOUR), 3);
        assert_eq!(w.unread(Filter::Snoozed, NOW + HOUR), 0);
    }

    #[test]
    fn snoozed_items_leave_the_inbox_and_show_in_their_filter() {
        let mut w = ws();
        assert!(w.snooze("pr87", Some(NOW + HOUR)));
        assert!(!w.snooze("gibt-es-nicht", None));
        assert!(!ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
        assert_eq!(ids(w.attention(Filter::Snoozed, NOW)), ["pr87"]);
        assert_eq!(
            ids(w.search(Filter::Snoozed, SourceFilter::GitHub, "slider", NOW)),
            ["pr87"]
        );
        assert!(
            w.search(Filter::Snoozed, SourceFilter::Jira, "", NOW)
                .is_empty()
        );
        // Search and source work on the rules, not on all entries.
        assert!(
            w.search(Filter::All, SourceFilter::All, "pr85", NOW)
                .is_empty()
        );
        // Snoozing again replaces the old snooze.
        w.snooze("pr87", None);
        assert_eq!(w.snoozed.len(), 1);
        assert!(w.unsnooze("pr87"));
        assert!(!w.unsnooze("pr87"));
        assert!(ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
    }

    #[test]
    fn wake_removes_only_what_expired() {
        let mut w = ws();
        w.snooze("pr87", Some(NOW + HOUR));
        w.snooze("bau150", Some(NOW + 5 * HOUR));
        w.snooze("bau138", None);
        assert!(w.wake(NOW).is_empty());
        assert_eq!(w.wake(NOW + 2 * HOUR), ["pr87"]);
        assert_eq!(w.snoozed.len(), 2);
        // „Until something changes“: a comment brings the entry back.
        w.reply("bau138", "Hallo");
        assert!(ids(w.attention(Filter::All, NOW)).contains(&"bau138"));
        assert_eq!(w.expired_snoozes(NOW), ["bau138"]);
        assert_eq!(w.wake(NOW), ["bau138"]);
        assert_eq!(w.snoozed.len(), 1);
    }

    #[test]
    fn a_status_change_wakes_a_change_snooze() {
        let mut w = ws();
        w.snooze("bau150", None);
        assert!(w.is_snoozed("bau150", NOW + 1000 * HOUR));
        w.set_status("bau150", TicketStatus::InProgress);
        assert!(!w.is_snoozed("bau150", NOW));
    }

    #[test]
    fn answering_a_review_request_ends_the_attention() {
        let mut w = ws();
        for (s, d) in [
            ("a", Decision::Approved),
            ("b", Decision::Approved),
            ("c", Decision::Approved),
        ] {
            w.decide(87, s, Some(d));
        }
        w.send_review(87, "").unwrap();
        assert!(!ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
    }

    #[test]
    fn only_tickets_have_a_status() {
        let mut w = ws();
        assert!(w.set_status("bau150", TicketStatus::Done));
        assert_eq!(w.item("bau150").unwrap().status, TicketStatus::Done);
        assert!(!w.set_status("pr87", TicketStatus::Done));
        assert!(!w.set_status("gibt-es-nicht", TicketStatus::Done));
    }

    #[test]
    fn branch_is_named_after_key_and_title() {
        let mut w = ws();
        assert_eq!(
            w.create_branch("bau150").as_deref(),
            Some("feature/BAU-150-sheet-detents-mittel-und")
        );
        assert_eq!(w.create_branch("bau150"), None, "es gibt schon einen");
        assert_eq!(w.create_branch("pr87"), None);
    }

    #[test]
    fn replies_are_trimmed_and_mine() {
        let mut w = ws();
        assert!(!w.reply("bau142", "  \n "));
        assert!(w.reply("bau142", "  Mache ich.  "));
        let last = w.item("bau142").unwrap().comments.last().unwrap();
        assert_eq!(
            (last.text.as_str(), last.own, last.author.name.as_str()),
            ("Mache ich.", true, "Du")
        );
    }

    #[test]
    fn sending_a_review_closes_the_inbox_item() {
        let mut w = ws();
        assert_eq!(w.send_review(87, ""), Err(SendError::Undecided(3)));
        assert_eq!(w.send_review(1, ""), Err(SendError::UnknownReview));
        w.decide(87, "a", Some(Decision::Approved));
        w.decide(87, "b", Some(Decision::Declined));
        w.decide(87, "c", Some(Decision::Approved));
        w.set_verdict(87, Verdict::RequestChanges);
        assert_eq!(w.send_review(87, " Bitte Fokus prüfen. "), Ok(2));

        let item = w.item("pr87").unwrap();
        let comment = item.comments.last().unwrap();
        assert_eq!(
            comment.text,
            "Review gesendet: Änderungen · 2 Kommentare\nBitte Fokus prüfen."
        );
        assert!(comment.own && item.read);
        assert_eq!(item.agent_note, None);
        assert_eq!(w.open_reviews(), 2);
    }

    #[test]
    fn own_line_comments_go_out_with_the_review() {
        let mut w = ws();
        let file = w.review(87).unwrap().files[0].name.clone();
        assert_eq!(w.add_line_comment(1, &file, 0, "x", None), None);
        let id = w
            .add_line_comment(87, &file, 0, "Warum hier?", None)
            .unwrap();
        for s in ["a", "b", "c"] {
            w.decide(87, s, Some(Decision::Declined));
        }
        // Own comments alone count too, even if all suggestions are declined.
        assert_eq!(w.send_review(87, ""), Ok(1));
        assert_eq!(
            w.item("pr87").unwrap().comments.last().unwrap().text,
            "Review gesendet: Kommentieren · 1 Kommentar"
        );
        assert!(w.review(87).unwrap().sent);
        assert!(w.edit_line_comment(87, &id, "Anders", None));
        assert!(!w.review(87).unwrap().sent);
        assert!(w.delete_line_comment(87, &id));
        assert!(!w.delete_line_comment(1, &id));
    }

    #[test]
    fn agent_review_prepares_suggestions() {
        let mut w = ws();
        assert!(!w.review(85).unwrap().agent_reviewed);
        let count = w.request_agent_review(85);
        assert!(count >= 1);
        assert!(w.review(85).unwrap().agent_reviewed);
        assert!(
            w.item("pr85")
                .unwrap()
                .agent_note
                .as_ref()
                .unwrap()
                .contains("vom Agenten")
        );
        assert_eq!(w.send_review(85, ""), Err(SendError::Undecided(count)));
    }

    #[test]
    fn an_agent_turns_a_ticket_into_a_pull_request() {
        let mut w = ws();
        assert_eq!(w.next_pr_number(), 92);
        w.start_agent("BAU-142", direct()).unwrap();
        assert_eq!(w.running_agents(), 1);
        assert_eq!(w.item("bau142").unwrap().status, TicketStatus::InProgress);
        assert_eq!(
            w.start_agent("BAU-142", direct()),
            Err(AgentError::AlreadyRunning)
        );

        assert_eq!(finish_agents(&mut w), vec!["BAU-142".to_string()]);
        assert_eq!(w.running_agents(), 0);

        let item = w.item("bau142").unwrap();
        assert_eq!(
            item.branch.as_deref(),
            Some("claude/BAU-142-toggle-fokus-ring-fehlt")
        );
        assert_eq!(item.pr.as_ref().unwrap().label(), "#92 · Entwurf");
        assert_eq!(item.status, TicketStatus::Review);
        assert_eq!(item.reason, ReasonKind::AgentFinished);
        assert!(!item.read, "a finished run needs attention");

        let review = w.review(92).unwrap();
        assert!(review.author.is_agent());
        assert_eq!(review.files.len(), 2);
        assert_eq!(review.ticket_key.as_deref(), Some("BAU-142"));
        assert_eq!(w.next_pr_number(), 93);
    }

    #[test]
    fn two_agents_get_different_pull_requests() {
        let mut w = ws();
        w.start_agent("BAU-142", direct()).unwrap();
        w.start_agent("BAU-150", direct()).unwrap();
        assert_eq!(w.running_agents(), 2);
        let mut finished = finish_agents(&mut w);
        finished.sort();
        assert_eq!(finished, ["BAU-142", "BAU-150"]);
        assert_eq!(w.item("bau142").unwrap().pr.as_ref().unwrap().number, 92);
        assert_eq!(w.item("bau150").unwrap().pr.as_ref().unwrap().number, 93);
    }

    #[test]
    fn options_decide_draft_and_checks() {
        let mut w = ws();
        let options = AgentOptions {
            run_tests: false,
            draft_pr: false,
            ..direct()
        };
        w.start_agent("BAU-150", options).unwrap();
        finish_agents(&mut w);
        let item = w.item("bau150").unwrap();
        assert_eq!(
            item.pr,
            Some(PrRef {
                number: 92,
                draft: false,
                checks: Checks::Running,
                changes_requested: false,
                merged: false,
                review_requested: true,
            })
        );
        assert_eq!(
            item.agent_note.as_deref(),
            Some("Vom Agenten umgesetzt · Tests übersprungen")
        );
    }

    #[test]
    fn a_cancelled_agent_leaves_no_traces() {
        let mut w = ws();
        w.start_agent("BAU-142", direct()).unwrap();
        w.advance_agents();
        w.cancel_agent("BAU-142");
        assert_eq!(w.running_agents(), 0);
        assert!(finish_agents(&mut w).is_empty());
        let item = w.item("bau142").unwrap();
        assert_eq!(
            (
                item.branch.clone(),
                item.pr.clone(),
                item.agent_note.clone()
            ),
            (None, None, None)
        );
        assert!(w.review(92).is_none());
        // A finished run can no longer be cancelled.
        w.cancel_agent("BAU-138");
        assert!(w.run("BAU-138").unwrap().is_done());
    }

    #[test]
    fn a_plan_is_shown_before_anything_happens() {
        let mut w = ws();
        w.start_agent("BAU-142", AgentOptions::default()).unwrap();
        assert_eq!((w.running_agents(), w.waiting_agents()), (0, 1));
        assert_eq!(
            w.start_agent("BAU-142", AgentOptions::default()),
            Err(AgentError::AlreadyRunning)
        );
        // The tick makes no progress without approval.
        assert!(finish_agents(&mut w).is_empty());
        let run = w.run("BAU-142").unwrap();
        assert!(run.is_awaiting());
        assert_eq!(run.completed, 0);
        // Neither branch nor pull request nor new status.
        let item = w.item("bau142").unwrap();
        assert_eq!((item.branch.clone(), item.pr.clone()), (None, None));
        assert_eq!(item.status, ws().item("bau142").unwrap().status);
        assert_eq!(
            item.agent_note.as_deref(),
            Some("Der Agent wartet auf die Freigabe seines Plans")
        );
        assert!(w.review(92).is_none());
    }

    #[test]
    fn approving_continues_to_the_pull_request() {
        let mut w = ws();
        w.start_agent("BAU-142", AgentOptions::default()).unwrap();
        assert!(w.approve_plan("BAU-142"));
        assert!(!w.approve_plan("BAU-142"), "approve only once");
        assert_eq!((w.running_agents(), w.waiting_agents()), (1, 0));
        assert_eq!(finish_agents(&mut w), vec!["BAU-142".to_string()]);
        assert_eq!(w.item("bau142").unwrap().pr.as_ref().unwrap().number, 92);
        assert!(w.review(92).is_some());
    }

    #[test]
    fn rejecting_a_plan_leaves_no_traces() {
        let mut w = ws();
        let before = w.clone();
        w.start_agent("BAU-142", AgentOptions::default()).unwrap();
        assert!(w.reject_plan("BAU-142"));
        assert!(!w.reject_plan("BAU-142"));
        assert_eq!((w.running_agents(), w.waiting_agents()), (0, 0));
        assert!(finish_agents(&mut w).is_empty());
        assert_eq!(w.item("bau142"), before.item("bau142"));
        assert!(w.review(92).is_none());
        assert!(w.run("BAU-142").unwrap().is_idle());
        // After that it can be restarted.
        assert!(w.start_agent("BAU-142", AgentOptions::default()).is_ok());
    }

    #[test]
    fn a_waiting_plan_can_be_cancelled_and_revised() {
        let mut w = ws();
        w.start_agent("BAU-150", AgentOptions::default()).unwrap();
        assert!(!w.revise_plan("BAU-150", "  "), "empty does not count");
        assert!(w.revise_plan("BAU-150", "Auch die Doku anpassen"));
        assert!(w.run("BAU-150").unwrap().is_awaiting());
        w.cancel_agent("BAU-150");
        assert!(w.run("BAU-150").unwrap().is_idle());
        assert_eq!(w.item("bau150").unwrap().agent_note, None);
        assert!(!w.revise_plan("BAU-150", "zu spät"));
    }

    #[test]
    fn agents_only_work_on_tickets() {
        let mut w = ws();
        assert_eq!(
            w.start_agent("#87", direct()),
            Err(AgentError::UnknownTicket)
        );
        assert_eq!(
            w.start_agent("NOPE-1", direct()),
            Err(AgentError::UnknownTicket)
        );
        assert!(w.run("#87").is_none());
        let fresh = w.run("BAU-150").unwrap();
        assert!(fresh.is_idle());
        assert_eq!(fresh.files.len(), 2);
    }

    #[test]
    fn an_agent_pull_request_without_tests_gets_green_when_its_check_finishes() {
        let mut w = ws();
        let options = AgentOptions {
            run_tests: false,
            ..direct()
        };
        w.start_agent("BAU-150", options).unwrap();
        finish_agents(&mut w);
        assert_eq!(w.review(92).unwrap().checks, Checks::Running);
        assert_eq!(w.running_checks(), 1);
        assert_eq!(w.merge_blockers(92)[0], MergeBlocker::Draft);
        for _ in 0..CHECK_STEPS {
            w.advance_checks();
        }
        assert_eq!(w.review(92).unwrap().checks, Checks::Passed);
        assert_eq!(
            w.item("bau150").unwrap().pr.as_ref().unwrap().checks,
            Checks::Passed
        );
    }
}
