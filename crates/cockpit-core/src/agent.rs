//! An agent implements a ticket: read, branch, changes, tests, pull request.
//!
//! The run is a small state machine. Whoever drives it – in the demo a
//! timer, later a backend – only calls [`AgentRun::advance`].
//!
//! On request the agent first presents a [`Plan`] and waits for approval
//! ([`RunState::AwaitingApproval`]). Until then nothing comes into being: no branch, no change.
//!
//! During the run the agent can ask a [`Question`] ([`RunState::AwaitingAnswer`]),
//! the user can send it hints ([`AgentRun::steer`]) and pause it
//! ([`RunState::Paused`]). Everything said is in the conversation history ([`AgentRun::messages`]).
//!
//! Besides the run „implement ticket“ there are two follow-up jobs ([`RunKind`]): implementing review comments
//! (new commit on the same branch, no new pull request) and investigating a red check.
//! The investigation stops after the [`Diagnosis`] ([`RunState::AwaitingFix`]) and
//! does nothing before you adopt the fix.
//!
//! Every step has a consumption ([`AgentRun::step_usage`]). If a run reaches a limit
//! ([`AgentLimits`]), it stops ([`RunState::Stopped`]) and waits for you to raise the limit
//! and resume it – or cancel. The rules of the repo ([`RepoRules`]) apply from the
//! start: they appear in the plan, and a plan that touches forbidden paths cannot be approved.

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

use crate::diagnosis::Diagnosis;
use crate::model::PrKey;
use crate::rules::{PathViolation, RepoRules};
use crate::text::slug;
use crate::usage::{AgentLimits, StopReason, Usage};

mod options;
pub use options::*;

mod repos;
pub use repos::*;

mod types;
pub use types::*;

/// Number of steps of a run.
pub const STEPS: usize = 5;

/// After this many completed steps the investigation presents the diagnosis.
const DIAGNOSED_AT: usize = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentRun {
    #[serde(default)]
    pub ticket_key: String,
    #[serde(default)]
    pub ticket_title: String,
    /// Comments on the ticket that the agent reads along.
    #[serde(default)]
    pub comments: usize,
    /// Names of the files the agent changes.
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub options: AgentOptions,
    #[serde(default)]
    pub state: RunState,
    /// Number of completed steps (0 to [`STEPS`]).
    #[serde(default)]
    pub completed: usize,
    /// Number of the pull request the run creates (in the repo `repo`).
    #[serde(default)]
    pub pr_number: Option<u32>,
    /// Notes of the user that led to a new plan.
    #[serde(default)]
    pub plan_notes: Vec<String>,
    /// The run began with a plan (also after approval).
    #[serde(default)]
    pub planned: bool,
    /// Conversation history: messages of the agent, questions, answers and hints.
    #[serde(default)]
    pub messages: Vec<RunMessage>,
    /// The open question, as long as the run waits for an answer.
    #[serde(default)]
    pub question: Option<Question>,
    /// The question was already asked in this run (it comes only once).
    #[serde(default)]
    pub asked: bool,
    /// Hints the agent takes into account in the next step.
    #[serde(default)]
    pub pending_hints: Vec<String>,
    /// What the run does. Old states know only „implement ticket“.
    #[serde(default)]
    pub kind: RunKind,
    /// Branch of the pull request a follow-up job pushes to. Empty for „implement ticket“.
    #[serde(default)]
    pub target_branch: String,
    /// The diagnosis of an investigation, once it is available.
    #[serde(default)]
    pub diagnosis: Option<Diagnosis>,
    /// The repo (`owner/name`) the run works in: where it clones and pushes, and the key of
    /// the rules. Empty in old states (then the only configured repo applies).
    #[serde(default)]
    pub repo: String,
    /// All repos of the run (the first is `repo`), each with its branch settings and result.
    /// Old states have none: see [`AgentRun::repo_list`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<RunRepo>,
    /// The rules of the repo as they applied at start (or at planning).
    #[serde(default)]
    pub rules: RepoRules,
    /// Consumption per completed step (index = step).
    #[serde(default)]
    pub step_usage: Vec<Usage>,
    /// Additionally allowed budget of this run on resume, in cents.
    #[serde(default)]
    pub extra_budget_cents: u64,
    /// Additionally allowed minutes of this run on resume.
    #[serde(default)]
    pub extra_minutes: u64,
    /// The run belongs to the real agent (container); the simulated tick does not touch it.
    #[serde(default)]
    pub real: bool,
    /// The plan from the container. As long as it is missing, the agent is still working on it.
    #[serde(default)]
    pub agent_plan: Option<AgentPlan>,
    /// Why the last run ended without result (plain text); empty as long as everything went well.
    #[serde(default)]
    pub ended: Option<String>,
    /// The agent's result, when it is done.
    #[serde(default)]
    pub summary: String,
    /// The pull request that was created (or found) for the branch of this run, after you
    /// approved it. `None` until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequestRef>,
    /// The pull request of the run is merged or closed: the run is „erledigt“. Old states have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure: Option<Closure>,
}

impl AgentRun {
    pub fn new(ticket_key: &str, ticket_title: &str) -> Self {
        Self {
            ticket_key: ticket_key.into(),
            ticket_title: ticket_title.into(),
            comments: 0,
            files: Vec::new(),
            options: AgentOptions::default(),
            state: RunState::Idle,
            completed: 0,
            pr_number: None,
            plan_notes: Vec::new(),
            planned: false,
            messages: Vec::new(),
            question: None,
            asked: false,
            pending_hints: Vec::new(),
            kind: RunKind::Implement,
            target_branch: String::new(),
            diagnosis: None,
            repo: String::new(),
            repos: Vec::new(),
            rules: RepoRules::default(),
            step_usage: Vec::new(),
            extra_budget_cents: 0,
            extra_minutes: 0,
            real: false,
            agent_plan: None,
            ended: None,
            summary: String::new(),
            pull_request: None,
            closure: None,
        }
    }

    /// A follow-up job on an existing pull request. `item_key` is the entry the pull request
    /// hangs on, `branch` its branch.
    pub fn followup(kind: RunKind, item_key: &str, title: &str, branch: &str) -> Self {
        let mut run = Self::new(item_key, title);
        run.pr_number = kind.review().map(|k| k.number);
        if let Some(k) = kind.review() {
            run.repo = k.repo;
        }
        run.kind = kind;
        run.target_branch = branch.into();
        run
    }

    /// The key of the run under which pages, actions and log find it:
    /// for „implement ticket“ the ticket key, otherwise `review-91` or `check-85-lint`.
    pub fn key(&self) -> String {
        match &self.kind {
            RunKind::Implement => self.ticket_key.clone(),
            RunKind::AddressReview { review, .. } => format!("review-{}", review.slug()),
            RunKind::Diagnose { review, check } => {
                format!("check-{}-{}", review.slug(), slug(check, 24))
            }
            RunKind::Followup { review, task } => match task {
                FollowupTask::ReviewThreads => format!("threads-{}", review.slug()),
                FollowupTask::FailedChecks { .. } => format!("fixes-{}", review.slug()),
            },
            RunKind::ReviewPr { review } => format!("prreview-{}", review.slug()),
        }
    }

    /// The pull request of the run (the one it creates or works on), once it has a number.
    /// The repo is the one the run works in.
    pub fn pr_key(&self) -> Option<PrKey> {
        self.pr_number.map(|n| PrKey::new(&self.repo, n))
    }

    /// Short name for messages: the key, for follow-up jobs a short text.
    pub fn label_msg(&self) -> Msg {
        match &self.kind {
            RunKind::Implement => Msg::raw(self.ticket_key.clone()),
            RunKind::AddressReview { review, .. } => {
                msg!(CoreAgentLabelAddress, review = review.to_string())
            }
            RunKind::Diagnose { review, check } => msg!(
                CoreAgentLabelDiagnose,
                check = check,
                review = review.to_string()
            ),
            RunKind::Followup { review, task } => match task {
                FollowupTask::ReviewThreads => {
                    msg!(CoreAgentLabelThreads, review = review.to_string())
                }
                FollowupTask::FailedChecks { .. } => {
                    msg!(CoreAgentLabelChecks, review = review.to_string())
                }
            },
            RunKind::ReviewPr { review } => {
                msg!(CoreAgentLabelReview, review = review.to_string())
            }
        }
    }

    /// [`AgentRun::label_msg`] in German (plain-text readers, tests).
    pub fn label(&self) -> String {
        self.label_msg().de()
    }

    /// The pull request that decides when the run is „erledigt“: the one that was created or
    /// found for an implementation, the one a follow-up job works on. `None` while there is none
    /// or the repo is unknown (old states).
    pub fn related_pr(&self) -> Option<PrKey> {
        if self.repo.is_empty() {
            return None;
        }
        match &self.kind {
            // With several repos the first pull request decides; the others follow on their own.
            RunKind::Implement => self
                .pull_requests()
                .into_iter()
                .next()
                .map(|(repo, p)| PrKey::new(&repo, p.number)),
            kind => kind.review(),
        }
    }

    /// The pull request to ask GitHub about: the run is not yet „erledigt“ and does not work
    /// right now (done, ready, paused or halted).
    pub fn pr_to_watch(&self) -> Option<PrKey> {
        let quiet = self.is_done() || self.is_idle() || self.is_paused() || self.is_stopped();
        if self.closure.is_some() || !quiet {
            return None;
        }
        self.related_pr()
    }

    /// The run is „erledigt“ (its pull request is merged or closed).
    pub fn is_closed(&self) -> bool {
        self.closure.is_some()
    }

    /// Marks the run as „erledigt“. A paused or halted run is ended first. `false` if the run is
    /// not waiting for this (see [`AgentRun::pr_to_watch`]).
    pub fn close(&mut self, closure: Closure) -> bool {
        if self.pr_to_watch().is_none() {
            return false;
        }
        if self.is_active() {
            self.reset();
        }
        self.closure = Some(closure);
        let text = match closure {
            Closure::Merged => msg!(CoreAgentMClosedMerged),
            Closure::Closed => msg!(CoreAgentMClosedClosed),
        };
        self.say_msg(Speaker::Agent, MessageKind::Info, text);
        true
    }

    pub fn is_followup(&self) -> bool {
        self.kind != RunKind::Implement
    }

    pub fn is_idle(&self) -> bool {
        self.state == RunState::Idle
    }

    pub fn is_awaiting(&self) -> bool {
        self.state == RunState::AwaitingApproval
    }

    /// Waits for the answer to a follow-up question.
    pub fn is_asking(&self) -> bool {
        self.state == RunState::AwaitingAnswer
    }

    pub fn is_paused(&self) -> bool {
        self.state == RunState::Paused
    }

    /// A limit has stopped the run.
    pub fn is_stopped(&self) -> bool {
        matches!(self.state, RunState::Stopped(_))
    }

    /// Which limit has stopped the run.
    pub fn stop_reason(&self) -> Option<StopReason> {
        match self.state {
            RunState::Stopped(reason) => Some(reason),
            _ => None,
        }
    }

    /// Waits, asks, is paused, is stopped or running – the run occupies the ticket.
    pub fn is_active(&self) -> bool {
        self.is_awaiting()
            || self.is_running()
            || self.is_asking()
            || self.is_paused()
            || self.is_diagnosed()
            || self.is_stopped()
    }

    /// The agent works on the steps (running, asking, paused, stopped or waiting
    /// for the decision about its fix).
    fn is_working(&self) -> bool {
        self.is_running()
            || self.is_asking()
            || self.is_paused()
            || self.is_diagnosed()
            || self.is_stopped()
    }

    /// The diagnosis is ready; the agent waits for „Fix übernehmen“ or „Verwerfen“.
    pub fn is_diagnosed(&self) -> bool {
        self.state == RunState::AwaitingFix
    }

    /// Is the run waiting for a decision from you (plan, answer or fix)?
    pub fn is_waiting_for_you(&self) -> bool {
        self.is_awaiting() || self.is_asking() || self.is_diagnosed()
    }

    /// The run currently consumes resources: it works or waits in the middle of a step for an
    /// answer. Only these runs count for the limit of parallel runs.
    pub fn is_consuming(&self) -> bool {
        self.is_running() || self.is_asking()
    }

    /// Consumption of the whole run (sum of the steps).
    pub fn usage(&self) -> Usage {
        let mut sum = Usage::default();
        for u in &self.step_usage {
            sum.add(u);
        }
        sum
    }

    /// Enters the consumption of the step the agent is currently working on. In the demo
    /// the tick does this; the real agent reports the values from the run here. Several reports for
    /// the same step add up.
    pub fn record_usage(&mut self, usage: Usage) {
        while self.step_usage.len() <= self.completed {
            self.step_usage.push(Usage::default());
        }
        self.step_usage[self.completed].add(&usage);
    }

    /// Files of the run that violate the path rules.
    pub fn violations(&self) -> Vec<PathViolation> {
        self.rules.violations(&self.files)
    }

    pub fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    pub fn is_done(&self) -> bool {
        self.state == RunState::Done
    }

    /// `claude/BAU-142-toggle-fokus-ring-fehlt` (or whatever the pattern of the repo makes of it)
    pub fn branch(&self) -> String {
        self.branch_in(&self.repo)
    }
}

mod lifecycle;
mod log;
mod plan;
mod report;
mod steps;
mod texts;

#[cfg(test)]
mod tests;
