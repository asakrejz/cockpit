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

use serde::{Deserialize, Serialize};

use crate::diagnosis::Diagnosis;
use crate::model::PrKey;
use crate::rules::{PathViolation, RepoRules};
use crate::text::slug;
use crate::usage::{AgentLimits, StopReason, Usage};

/// Number of steps of a run.
pub const STEPS: usize = 5;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct AgentOptions {
    pub run_tests: bool,
    /// Create the pull request as a draft.
    pub draft_pr: bool,
    /// Additional hints to the agent.
    pub hint: String,
    pub base_branch: String,
    /// First present a plan and wait for approval.
    pub plan_first: bool,
    /// The repo (`owner/name`) the run works in. `None` = the backend chooses by the rules of
    /// the configuration; the interface sets it when the user picks one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            run_tests: true,
            draft_pr: true,
            hint: String::new(),
            base_branch: "main".into(),
            plan_first: true,
            repo: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunState {
    #[default]
    Idle,
    /// The plan is ready, the agent waits for the user.
    AwaitingApproval,
    Running,
    /// The agent asked a question and waits for the answer.
    AwaitingAnswer,
    /// The user has paused the run.
    Paused,
    /// The diagnosis of a red check is ready; the agent waits whether it should adopt the fix.
    AwaitingFix,
    /// A limit (budget, time limit) is reached: the run waits for you.
    Stopped(StopReason),
    Done,
}

/// What the run does.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum RunKind {
    /// Implement a ticket and create a pull request.
    #[default]
    Implement,
    /// Implement comments of a review and push to the pull request's branch.
    AddressReview {
        /// The pull request (repo and number).
        review: PrKey,
        /// IDs of the line comments the agent is to implement.
        comments: Vec<String>,
    },
    /// Investigate a red check, present a diagnosis and – after your approval –
    /// push the fix.
    Diagnose {
        /// The pull request (repo and number).
        review: PrKey,
        /// Name of the check.
        check: String,
    },
    /// Follow-up job of the **real** agent on the branch of an existing pull request: the
    /// backend reads the inputs (review threads, failed checks) from GitHub when the run starts and
    /// hands them to the container. The agent continues on the same `claude/…` branch; the backend
    /// pushes the new commits (never a force-push).
    Followup {
        /// The pull request (repo and number).
        review: PrKey,
        task: FollowupTask,
    },
}

/// What a real follow-up job does.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FollowupTask {
    /// Implement the review comments of the pull request.
    ReviewThreads,
    /// Fix red checks (names of the checks that were red when the job was commissioned).
    FailedChecks { checks: Vec<String> },
}

impl FollowupTask {
    /// Id used in the job file and in run keys.
    pub fn id(&self) -> &'static str {
        match self {
            FollowupTask::ReviewThreads => "review_comments",
            FollowupTask::FailedChecks { .. } => "failed_checks",
        }
    }
}

impl RunKind {
    /// The pull request a follow-up job works on.
    pub fn review(&self) -> Option<PrKey> {
        match self {
            RunKind::Implement => None,
            RunKind::AddressReview { review, .. }
            | RunKind::Diagnose { review, .. }
            | RunKind::Followup { review, .. } => Some(review.clone()),
        }
    }

    /// Does the real agent run this job in a container (as opposed to the simulation)?
    pub fn runs_in_container(&self) -> bool {
        matches!(self, RunKind::Implement | RunKind::Followup { .. })
    }
}

/// After this many completed steps the investigation presents the diagnosis.
const DIAGNOSED_AT: usize = 3;

/// A pull request on GitHub that belongs to the branch of a run.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PullRequestRef {
    pub number: u32,
    /// Web address of the pull request.
    #[serde(default)]
    pub url: String,
    /// It already existed for the branch (nothing was created, it was only linked).
    #[serde(default)]
    pub existed: bool,
}

/// A follow-up question of the agent, optionally with proposed answers.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Question {
    pub text: String,
    #[serde(default)]
    pub suggestions: Vec<String>,
}

/// The plan the real agent presented from the container (instead of the derived one).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentPlan {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
}

/// What the real agent reports from the container. The backend checks the container's lines and
/// passes them on as `Action::ReportAgent` (author Agent); `cockpit-core` decides what they
/// effect – the agent can change the state only through these paths.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AgentReport {
    /// The container is running. `detail` is a sentence for the history (runtime, mode).
    Started { detail: String },
    /// The plan is ready (only while the run waits for approval).
    Plan {
        summary: String,
        files: Vec<String>,
        steps: Vec<String>,
    },
    /// A work step with its consumption.
    Step {
        title: String,
        text: String,
        input_tokens: u64,
        output_tokens: u64,
        millis: u64,
    },
    /// The files the agent has changed.
    Files { files: Vec<String> },
    /// A follow-up question; the run waits for the answer.
    Question {
        text: String,
        #[serde(default)]
        suggestions: Vec<String>,
    },
    /// This many hints of the user (the oldest) have arrived at the agent.
    HintsDelivered { count: usize },
    /// Done: the branch is pushed.
    Done { summary: String },
    /// The run ended without result (error, timeout, restart). `reason` in plain text.
    Failed { reason: String },
}

/// What a report of the agent has effected; the workspace books and logs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportEffect {
    /// Sentences for the log.
    pub notes: Vec<String>,
    /// New consumption for the ledger.
    pub usage: Option<Usage>,
    /// The run has finished.
    pub finished: bool,
    /// The run was ended and reset to ready to start.
    pub failed: bool,
    /// A limit has stopped the run.
    pub stopped: Option<StopReason>,
    /// The run now waits for an answer.
    pub asking: bool,
}

/// Who speaks in the history.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Agent,
    #[serde(alias = "Du")]
    User,
}

/// What a message in the history is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// Normal message (job, step completed).
    Info,
    Question,
    Answer,
    /// Hint of the user, also a note on the plan.
    Hint,
    /// The agent confirms that it takes a hint into account.
    Ack,
}

/// A line in the conversation history of a run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RunMessage {
    pub from: Speaker,
    pub kind: MessageKind,
    /// Number of completed steps at the time the message arose.
    pub step: usize,
    pub text: String,
}

/// What a tick has effected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// The run is not running, nothing happened.
    Idle,
    /// The agent has stopped and asked a question.
    Asked,
    /// The investigation has presented the diagnosis and waits for the decision.
    Diagnosed,
    /// A step was completed.
    Stepped,
    /// The last step was completed.
    Finished,
    /// A limit is reached: the run has stopped without making a step.
    Stopped,
}

/// What the agent intends to do. Derived from ticket, hints and files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub summary: String,
    pub files: Vec<String>,
    pub steps: Vec<String>,
    /// The rules of the repo that apply to this run (one line per rule).
    pub rules: Vec<String>,
    /// Files of the plan that violate the path rules. As long as there are any, the
    /// plan cannot be approved.
    pub violations: Vec<PathViolation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub title: String,
    pub text: String,
    pub state: StepState,
    /// Consumption of the step once it is completed.
    pub usage: Option<Usage>,
}

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
            rules: RepoRules::default(),
            step_usage: Vec::new(),
            extra_budget_cents: 0,
            extra_minutes: 0,
            real: false,
            agent_plan: None,
            ended: None,
            summary: String::new(),
            pull_request: None,
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
        }
    }

    /// The pull request of the run (the one it creates or works on), once it has a number.
    /// The repo is the one the run works in.
    pub fn pr_key(&self) -> Option<PrKey> {
        self.pr_number.map(|n| PrKey::new(&self.repo, n))
    }

    /// Short name for messages: the key, for follow-up jobs with plain text.
    pub fn label(&self) -> String {
        match &self.kind {
            RunKind::Implement => self.ticket_key.clone(),
            RunKind::AddressReview { review, .. } => format!("Kommentare in {review}"),
            RunKind::Diagnose { review, check } => format!("Check „{check}“ in {review}"),
            RunKind::Followup { review, task } => match task {
                FollowupTask::ReviewThreads => format!("Review-Kommentare in {review}"),
                FollowupTask::FailedChecks { .. } => format!("Rote Checks in {review}"),
            },
        }
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

    /// `claude/BAU-142-toggle-fokus-ring-fehlt`
    pub fn branch(&self) -> String {
        if !self.target_branch.is_empty() {
            return self.target_branch.clone();
        }
        format!(
            "claude/{}-{}",
            self.ticket_key,
            slug(&self.ticket_title, 24)
        )
    }
}

mod lifecycle;
mod log;
mod plan;
mod report;
mod steps;

#[cfg(test)]
mod tests;
