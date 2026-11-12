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
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            run_tests: true,
            draft_pr: true,
            hint: String::new(),
            base_branch: "main".into(),
            plan_first: true,
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
        /// Number of the pull request.
        review: u32,
        /// IDs of the line comments the agent is to implement.
        comments: Vec<String>,
    },
    /// Investigate a red check, present a diagnosis and – after your approval –
    /// push the fix.
    Diagnose {
        /// Number of the pull request.
        review: u32,
        /// Name des Checks.
        check: String,
    },
}

impl RunKind {
    /// The pull request a follow-up job works on.
    pub fn review(&self) -> Option<u32> {
        match self {
            RunKind::Implement => None,
            RunKind::AddressReview { review, .. } | RunKind::Diagnose { review, .. } => {
                Some(*review)
            }
        }
    }
}

/// After this many completed steps the investigation presents the diagnosis.
const DIAGNOSED_AT: usize = 3;

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
    #[serde(rename = "Du")]
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
    /// Number of the pull request the run creates.
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
    /// The repo the run works in (key of the rules).
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
        }
    }

    /// A follow-up job on an existing pull request. `item_key` is the entry the pull request
    /// hangs on, `branch` its branch.
    pub fn followup(kind: RunKind, item_key: &str, title: &str, branch: &str) -> Self {
        let mut run = Self::new(item_key, title);
        run.pr_number = kind.review();
        run.kind = kind;
        run.target_branch = branch.into();
        run
    }

    /// The key of the run under which pages, actions and log find it:
    /// for „implement ticket“ the ticket key, otherwise `review-91` or `check-85-lint`.
    pub fn key(&self) -> String {
        match &self.kind {
            RunKind::Implement => self.ticket_key.clone(),
            RunKind::AddressReview { review, .. } => format!("review-{review}"),
            RunKind::Diagnose { review, check } => {
                format!("check-{review}-{}", slug(check, 24))
            }
        }
    }

    /// Short name for messages: the key, for follow-up jobs with plain text.
    pub fn label(&self) -> String {
        match &self.kind {
            RunKind::Implement => self.ticket_key.clone(),
            RunKind::AddressReview { review, .. } => format!("Kommentare in #{review}"),
            RunKind::Diagnose { review, check } => format!("Check „{check}“ in #{review}"),
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

    pub fn start(&mut self, mut options: AgentOptions, pr_number: u32) {
        // The rules of the repo come first: required tests cannot be switched off, and whoever
        // violates the path rules or whose repo demands it always presents a plan.
        if !self.rules.required_tests.is_empty() {
            options.run_tests = true;
        }
        if self.rules.plan_required || !self.violations().is_empty() {
            options.plan_first = true;
        }
        // An investigation only changes something after your approval of the fix, a plan would be
        // redundant. Follow-up jobs do not create a new pull request.
        let investigate = matches!(self.kind, RunKind::Diagnose { .. });
        self.planned = options.plan_first && !investigate;
        self.state = if self.planned {
            RunState::AwaitingApproval
        } else {
            RunState::Running
        };
        self.options = options;
        self.completed = 0;
        self.pr_number = Some(self.kind.review().unwrap_or(pr_number));
        self.plan_notes.clear();
        self.diagnosis = None;
        self.agent_plan = None;
        self.ended = None;
        self.summary.clear();
        self.clear_usage();
        self.clear_conversation();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("Auftrag für {} angenommen.", self.label()),
        );
        let hint = self.options.hint.trim().to_string();
        if !hint.is_empty() {
            self.say(Speaker::User, MessageKind::Hint, hint);
        }
        if self.planned {
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                "Ich habe einen Plan vorgelegt und warte auf deine Freigabe.".into(),
            );
        }
    }

    fn clear_usage(&mut self) {
        self.step_usage.clear();
        self.extra_budget_cents = 0;
        self.extra_minutes = 0;
    }

    fn clear_conversation(&mut self) {
        self.messages.clear();
        self.question = None;
        self.asked = false;
        self.pending_hints.clear();
    }

    fn say(&mut self, from: Speaker, kind: MessageKind, text: String) {
        let step = self.completed;
        self.messages.push(RunMessage {
            from,
            kind,
            step,
            text,
        });
    }

    /// Cancels or resets: the run is ready to start again afterwards.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.completed = 0;
        self.pr_number = self.kind.review();
        self.plan_notes.clear();
        self.planned = false;
        self.diagnosis = None;
        self.agent_plan = None;
        self.summary.clear();
        self.real = false;
        self.clear_usage();
        self.clear_conversation();
    }

    /// Adopts the fix of the diagnosis: the agent applies it, pushes and restarts the check.
    /// `false` if no diagnosis is waiting.
    pub fn accept_fix(&mut self) -> bool {
        if !self.is_diagnosed() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Info, "Fix übernommen.".into());
        true
    }

    /// Approves the plan: from now on the agent works. `false` if there is nothing to approve.
    pub fn approve(&mut self) -> bool {
        // A plan that touches forbidden paths cannot be approved.
        if !self.is_awaiting() || !self.violations().is_empty() || self.is_drafting() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Info, "Plan freigegeben.".into());
        true
    }

    /// Rejects the plan. The run ends without traces, as with cancelling.
    pub fn reject(&mut self) -> bool {
        if !self.is_awaiting() {
            return false;
        }
        self.reset();
        true
    }

    /// Requests a new plan with a note. Empty notes do not count.
    pub fn revise(&mut self, note: &str) -> bool {
        let note = note.trim();
        if !self.is_awaiting() || note.is_empty() {
            return false;
        }
        self.plan_notes.push(note.to_string());
        self.say(
            Speaker::User,
            MessageKind::Hint,
            format!("Anmerkung zum Plan: {note}"),
        );
        // The real agent works out a new plan; until it is ready, no approval is possible.
        if self.real {
            self.agent_plan = None;
        }
        true
    }

    /// The real agent is still working out the plan: there is nothing to approve.
    pub fn is_drafting(&self) -> bool {
        self.is_awaiting() && self.real && self.agent_plan.is_none()
    }

    /// Answers the open question; afterwards the agent keeps working. Empty answers
    /// do not count.
    pub fn answer(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !self.is_asking() || text.is_empty() {
            return false;
        }
        self.question = None;
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Answer, text.to_string());
        true
    }

    /// Sends the agent a hint. It takes it into account in the next step.
    /// Possible as long as the run works on the steps (also paused or waiting).
    pub fn steer(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !self.is_working() || text.is_empty() {
            return false;
        }
        self.pending_hints.push(text.to_string());
        self.say(Speaker::User, MessageKind::Hint, text.to_string());
        true
    }

    /// Stops a running agent. An open question remains.
    pub fn pause(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        self.state = RunState::Paused;
        self.say(Speaker::User, MessageKind::Info, "Lauf pausiert.".into());
        true
    }

    /// Resumes a paused run.
    pub fn resume(&mut self) -> bool {
        if !self.is_paused() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Info, "Lauf fortgesetzt.".into());
        true
    }

    /// Resumes a stopped run. `extra` raises what stopped it: cents for the
    /// run's budget, minutes for the time limit. The workspace raises the monthly budget itself;
    /// here it is only the message. `false` if the run is not stopped.
    pub fn resume_after_stop(&mut self, extra: u64) -> bool {
        let Some(reason) = self.stop_reason() else {
            return false;
        };
        match reason {
            StopReason::RunBudget => {
                self.extra_budget_cents = self.extra_budget_cents.saturating_add(extra)
            }
            StopReason::Time => self.extra_minutes = self.extra_minutes.saturating_add(extra),
            StopReason::MonthBudget => {}
        }
        self.state = RunState::Running;
        let text = if extra == 0 {
            "Lauf fortgesetzt.".to_string()
        } else {
            format!(
                "Lauf fortgesetzt – {} um {} erhöht.",
                reason.raises(),
                reason.raise_label(extra)
            )
        };
        self.say(Speaker::User, MessageKind::Info, text);
        true
    }

    /// The current plan. Before the start there is none.
    pub fn plan(&self) -> Option<Plan> {
        if !self.planned {
            return None;
        }
        if let Some(p) = &self.agent_plan {
            return Some(Plan {
                summary: p.summary.clone(),
                files: p.files.clone(),
                steps: p.steps.clone(),
                rules: self.rules.lines(),
                violations: self.violations(),
            });
        }
        if let RunKind::AddressReview { review, comments } = &self.kind {
            return Some(self.address_plan(*review, comments.len()));
        }
        let o = &self.options;
        let hint = o.hint.trim();
        let files = match self.files.len() {
            1 => "1 Datei".to_string(),
            n => format!("{n} Dateien"),
        };
        let mut summary = format!(
            "{} umsetzen: {files} ändern{}.",
            self.ticket_key,
            if o.run_tests {
                ", Tests ausführen"
            } else {
                " (ohne Tests)"
            }
        );
        if !hint.is_empty() {
            summary.push_str(&format!(" Dein Hinweis: „{hint}“."));
        }
        let mut steps = vec![
            format!(
                "Ticket {} und {} Kommentar(e) auswerten",
                self.ticket_key, self.comments
            ),
            format!("Branch {} von {} erstellen", self.branch(), o.base_branch),
            format!("{} bearbeiten", self.files.join(", ")),
        ];
        for note in &self.plan_notes {
            steps.push(format!("Berücksichtigen: {note}"));
        }
        steps.push(if !self.rules.required_tests.is_empty() {
            format!(
                "Pflicht-Tests ausführen: {}",
                self.rules.required_tests.join(", ")
            )
        } else if o.run_tests {
            "Lint und Tests ausführen".into()
        } else {
            "Tests überspringen".into()
        });
        steps.push(format!(
            "Pull Request {}anlegen – ohne Merge",
            if o.draft_pr { "als Entwurf " } else { "" }
        ));
        if !self.plan_notes.is_empty() {
            summary.push_str(&format!(" Überarbeitet ({}×).", self.plan_notes.len()));
        }
        Some(Plan {
            summary,
            files: self.files.clone(),
            steps,
            rules: self.rules.lines(),
            violations: self.violations(),
        })
    }

    /// Plan for „implement review comments“: no new branch, no new pull request.
    fn address_plan(&self, review: u32, comments: usize) -> Plan {
        let o = &self.options;
        let hint = o.hint.trim();
        let branch = self.branch();
        let mut summary = format!(
            "{} aus Review #{review} umsetzen: {} ändern{}, auf {branch} pushen.",
            plural_comments(comments),
            files_label(self.files.len()),
            if o.run_tests {
                ", Tests ausführen"
            } else {
                " (ohne Tests)"
            }
        );
        if !hint.is_empty() {
            summary.push_str(&format!(" Dein Hinweis: „{hint}“."));
        }
        let mut steps = vec![
            format!("{} aus Review #{review} lesen", plural_comments(comments)),
            format!("Branch {branch} auschecken"),
            format!("{} bearbeiten", self.files.join(", ")),
        ];
        for note in &self.plan_notes {
            steps.push(format!("Berücksichtigen: {note}"));
        }
        steps.push(if !self.rules.required_tests.is_empty() {
            format!(
                "Pflicht-Tests ausführen: {}",
                self.rules.required_tests.join(", ")
            )
        } else if o.run_tests {
            "Lint und Tests ausführen".into()
        } else {
            "Tests überspringen".into()
        });
        steps.push(format!(
            "Auf {branch} pushen – kein neuer Pull Request, kein Merge"
        ));
        if !self.plan_notes.is_empty() {
            summary.push_str(&format!(" Überarbeitet ({}×).", self.plan_notes.len()));
        }
        Plan {
            summary,
            files: self.files.clone(),
            steps,
            rules: self.rules.lines(),
            violations: self.violations(),
        }
    }

    /// Takes a report of the real agent. `None` if it does not fit in the current state
    /// (the run is not active, the plan is not expected …): then nothing changes.
    /// The reports are the only way the container changes the run.
    pub fn report(
        &mut self,
        report: AgentReport,
        limits: &AgentLimits,
        month_spent_cents: u64,
    ) -> Option<ReportEffect> {
        if !self.is_active() {
            return None;
        }
        let mut effect = ReportEffect::default();
        match report {
            AgentReport::Started { detail } => {
                self.real = true;
                effect
                    .notes
                    .push(format!("{}: Container gestartet", self.label()));
                self.say(Speaker::Agent, MessageKind::Info, detail);
            }
            AgentReport::Plan {
                summary,
                files,
                steps,
            } => {
                if !self.is_awaiting() {
                    return None;
                }
                self.real = true;
                self.files = files.clone();
                self.agent_plan = Some(AgentPlan {
                    summary,
                    files,
                    steps,
                });
                self.say(
                    Speaker::Agent,
                    MessageKind::Info,
                    "Der Plan liegt vor. Ich warte auf deine Freigabe.".into(),
                );
                effect.notes.push(format!(
                    "Plan für {} vorgelegt, wartet auf Freigabe",
                    self.label()
                ));
            }
            AgentReport::Step {
                title,
                text,
                input_tokens,
                output_tokens,
                millis,
            } => {
                if !(self.is_working() || self.is_awaiting()) {
                    return None;
                }
                let usage = Usage::priced(input_tokens, output_tokens, millis, &limits.pricing);
                self.record_usage(usage);
                effect.usage = Some(usage);
                let line = if text.trim().is_empty() {
                    title.clone()
                } else {
                    format!("{title}: {text}")
                };
                self.say(Speaker::Agent, MessageKind::Info, line);
                if self.is_working() {
                    // The last step belongs to the result; only `Done` completes the run.
                    if self.completed + 1 < STEPS {
                        self.completed += 1;
                    }
                    effect.notes.push(format!("{}: {title}", self.label()));
                    if self.is_running()
                        && let Some(reason) =
                            limits.reached(self, month_spent_cents.saturating_add(usage.cost_cents))
                    {
                        self.state = RunState::Stopped(reason);
                        let detail = limits.describe(
                            reason,
                            self,
                            month_spent_cents.saturating_add(usage.cost_cents),
                        );
                        self.say(
                            Speaker::Agent,
                            MessageKind::Info,
                            format!(
                                "Angehalten: {}. {detail} Erhöhe {} und setze den Lauf fort – oder brich ihn ab.",
                                reason.label(),
                                reason.raises()
                            ),
                        );
                        effect.notes.push(format!(
                            "{}: Angehalten – {}",
                            self.label(),
                            reason.label()
                        ));
                        effect.stopped = Some(reason);
                    }
                }
            }
            AgentReport::Files { files } => {
                if !self.is_working() {
                    return None;
                }
                self.files = files;
            }
            AgentReport::Question { text, suggestions } => {
                if !self.is_running() || text.trim().is_empty() {
                    return None;
                }
                self.asked = true;
                self.state = RunState::AwaitingAnswer;
                self.say(Speaker::Agent, MessageKind::Question, text.clone());
                self.question = Some(Question { text, suggestions });
                effect.asking = true;
                effect.notes.push(format!(
                    "{}: Der Agent hat eine Frage und wartet auf deine Antwort",
                    self.label()
                ));
            }
            AgentReport::HintsDelivered { count } => {
                let count = count.min(self.pending_hints.len());
                for hint in self.pending_hints.drain(..count).collect::<Vec<_>>() {
                    self.say(
                        Speaker::Agent,
                        MessageKind::Ack,
                        format!("Hinweis erhalten: {hint}"),
                    );
                }
            }
            AgentReport::Done { summary } => {
                if !self.is_working() {
                    return None;
                }
                self.state = RunState::Done;
                self.completed = STEPS;
                self.summary = summary.clone();
                self.question = None;
                self.say(Speaker::Agent, MessageKind::Info, summary);
                effect.finished = true;
            }
            AgentReport::Failed { reason } => {
                let label = self.label();
                self.reset();
                self.ended = Some(reason.clone());
                self.say(
                    Speaker::Agent,
                    MessageKind::Info,
                    format!("Der Lauf endete ohne Ergebnis: {reason}"),
                );
                effect.failed = true;
                effect
                    .notes
                    .push(format!("{label}: Lauf beendet – {reason}"));
            }
        }
        Some(effect)
    }

    /// Completes the current step. Returns `true` if the run is
    /// finished with it.
    pub fn advance(&mut self) -> bool {
        self.tick() == Tick::Finished
    }

    /// A tick: the agent takes waiting hints into account, perhaps asks a
    /// question and otherwise completes the current step.
    pub fn tick(&mut self) -> Tick {
        self.tick_with(&AgentLimits::unlimited(), 0)
    }

    /// Like [`AgentRun::tick`], with limits. If a limit is reached before the step, the run
    /// stops (`Stopped`) and does nothing. `month_spent_cents` is what was spent in the month across all runs.
    /// The step's consumption is calculated by the tick according to the prices of the limits.
    pub fn tick_with(&mut self, limits: &AgentLimits, month_spent_cents: u64) -> Tick {
        if !self.is_running() || self.real {
            return Tick::Idle;
        }
        if let Some(reason) = limits.reached(self, month_spent_cents) {
            self.state = RunState::Stopped(reason);
            let detail = limits.describe(reason, self, month_spent_cents);
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                format!(
                    "Angehalten: {}. {detail} Erhöhe {} und setze den Lauf fort – oder brich ihn ab.",
                    reason.label(),
                    reason.raises()
                ),
            );
            return Tick::Stopped;
        }
        for hint in std::mem::take(&mut self.pending_hints) {
            self.say(
                Speaker::Agent,
                MessageKind::Ack,
                format!("Hinweis erhalten: {hint}"),
            );
        }
        // The follow-up question is hard-wired in the demo (see `demo::agent_question`);
        // a backend would get it from the real agent.
        if !self.asked
            && let Some((step, question)) = crate::demo::agent_question(&self.key())
            && step == self.completed
        {
            self.asked = true;
            self.state = RunState::AwaitingAnswer;
            self.say(Speaker::Agent, MessageKind::Question, question.text.clone());
            self.question = Some(question);
            return Tick::Asked;
        }
        let done = self.steps()[self.completed.min(STEPS - 1)].clone();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("{}: {}", done.title, done.text),
        );
        // Consumption of the step: made-up but fixed values in the demo.
        let (input, output, millis) = crate::demo::step_tokens(&self.ticket_key, self.completed);
        self.record_usage(Usage::priced(input, output, millis, &limits.pricing));
        self.completed += 1;
        if self.completed == STEPS {
            self.state = RunState::Done;
            Tick::Finished
        } else if self.completed == DIAGNOSED_AT && matches!(self.kind, RunKind::Diagnose { .. }) {
            // The diagnosis is ready. Until your decision the branch stays untouched.
            self.state = RunState::AwaitingFix;
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                "Die Diagnose liegt vor. Ich warte, ob ich den Fix übernehmen soll.".into(),
            );
            Tick::Diagnosed
        } else {
            Tick::Stepped
        }
    }

    fn pr_label(&self) -> String {
        self.pr_number
            .map_or("Pull Request".to_string(), |n| format!("#{n}"))
    }

    /// Title and text of each step, matching kind and options of the run.
    pub fn steps(&self) -> Vec<Step> {
        let texts = match &self.kind {
            RunKind::Implement => self.implement_texts(),
            RunKind::AddressReview { review, comments } => {
                self.address_texts(*review, comments.len())
            }
            RunKind::Diagnose { review, check } => self.diagnose_texts(*review, check),
        };
        self.build_steps(texts)
    }

    fn address_texts(&self, review: u32, comments: usize) -> [(&'static str, String); STEPS] {
        let o = &self.options;
        [
            (
                "Kommentare gelesen",
                format!(
                    "{} aus Review #{review} ausgewertet",
                    plural_comments(comments)
                ),
            ),
            ("Branch ausgecheckt", self.branch()),
            (
                "Änderungen umgesetzt",
                files_label(self.files.len()) + " angepasst",
            ),
            if o.run_tests {
                (
                    "Tests ausgeführt",
                    "Lint und Tests laufen im Branch".to_string(),
                )
            } else {
                (
                    "Tests übersprungen",
                    "In den Optionen abgeschaltet".to_string(),
                )
            },
            (
                "Auf Branch gepusht",
                format!(
                    "Neuer Commit auf {} – kein neuer Pull Request, das Review wartet wieder auf dich",
                    self.branch()
                ),
            ),
        ]
    }

    fn diagnose_texts(&self, review: u32, check: &str) -> [(&'static str, String); STEPS] {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        [
            (
                "Log gelesen",
                format!("Check „{check}“ von #{review}: Ausgabe ausgewertet"),
            ),
            (
                "Ursache eingegrenzt",
                "Fehlermeldung, Datei und Zeile bestimmt".to_string(),
            ),
            (
                "Diagnose erstellt",
                "Ursache und Vorschlag liegen zur Entscheidung vor".to_string(),
            ),
            if code {
                (
                    "Fix angewendet",
                    "Änderung im Branch, Lint und Tests laufen".to_string(),
                )
            } else {
                (
                    "Kein Code-Fix nötig",
                    "Der Branch bleibt unverändert".to_string(),
                )
            },
            if code {
                (
                    "Gepusht, Check neu gestartet",
                    format!("{} aktualisiert, „{check}“ läuft erneut", self.branch()),
                )
            } else {
                ("Check neu gestartet", format!("„{check}“ läuft erneut"))
            },
        ]
    }

    fn implement_texts(&self) -> [(&'static str, String); STEPS] {
        let o = &self.options;
        let mut files = match self.files.len() {
            1 => "1 Datei angepasst".to_string(),
            n => format!("{n} Dateien angepasst"),
        };
        if self.rules.has_path_rules() && self.violations().is_empty() {
            files.push_str(" · Pfad-Regeln eingehalten");
        }
        [
            (
                "Ticket gelesen",
                format!(
                    "Beschreibung und Kommentare aus {} ausgewertet",
                    self.ticket_key
                ),
            ),
            (
                "Branch erstellt",
                format!("{} von {}", self.branch(), o.base_branch),
            ),
            ("Änderungen umgesetzt", files),
            if !self.rules.required_tests.is_empty() {
                (
                    "Pflicht-Tests ausgeführt",
                    format!(
                        "{} im Branch bestanden",
                        self.rules.required_tests.join(", ")
                    ),
                )
            } else if o.run_tests {
                (
                    "Tests ausgeführt",
                    "Lint und Tests laufen im Branch".to_string(),
                )
            } else {
                (
                    "Tests übersprungen",
                    "In den Optionen abgeschaltet".to_string(),
                )
            },
            (
                "Pull Request erstellt",
                format!(
                    "{}{} – wartet auf dein Review",
                    self.pr_label(),
                    if o.draft_pr { " als Entwurf" } else { "" }
                ),
            ),
        ]
    }

    fn build_steps(&self, texts: [(&'static str, String); STEPS]) -> Vec<Step> {
        texts
            .into_iter()
            .enumerate()
            .map(|(i, (title, text))| {
                if i == 0 && self.is_awaiting() {
                    return Step {
                        title: "Plan liegt vor".to_string(),
                        text: if self.violations().is_empty() {
                            "Wartet auf deine Freigabe – bis dahin ändert der Agent nichts"
                        } else {
                            "Der Plan verstößt gegen die Regeln des Repos – er lässt sich nicht freigeben"
                        }
                        .to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed && self.is_diagnosed() {
                    return Step {
                        title: "Diagnose liegt vor".to_string(),
                        text: "Wartet auf deine Entscheidung – bis dahin bleibt der Branch unverändert"
                            .to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed && self.is_asking() {
                    return Step {
                        title: "Frage an dich".to_string(),
                        text: "Wartet auf deine Antwort".to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed && self.is_paused() {
                    return Step {
                        title: title.to_string(),
                        text: "Pausiert – setze den Lauf fort, damit es weitergeht".to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed
                    && let Some(reason) = self.stop_reason()
                {
                    return Step {
                        title: title.to_string(),
                        text: format!("Angehalten: {} – erhöhe das Limit oder brich ab", reason.label()),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                // If the user answered a question at this step, it is noted there.
                let text = match self
                    .messages
                    .iter()
                    .find(|m| m.kind == MessageKind::Answer && m.step == i)
                {
                    Some(answer) => format!("{text} · Deine Antwort: {}", answer.text),
                    None => text,
                };
                Step {
                    title: title.to_string(),
                    text,
                    usage: if i < self.completed {
                        self.step_usage.get(i).copied()
                    } else {
                        None
                    },
                    state: if i < self.completed {
                        StepState::Done
                    } else if i == self.completed && self.is_working() {
                        StepState::Active
                    } else {
                        StepState::Pending
                    },
                }
            })
            .collect()
    }

    /// The log of a real run: what the agent and you said, plus the consumption.
    fn real_log(&self) -> String {
        let mut lines: Vec<String> = self
            .messages
            .iter()
            .map(|m| {
                let who = match m.from {
                    Speaker::Agent => "Agent",
                    Speaker::User => "Du",
                };
                format!("› {who}: {}", m.text)
            })
            .collect();
        let usage = self.usage();
        if !usage.is_empty() {
            lines.push(format!("› Verbrauch: {}", usage.line()));
        }
        if lines.is_empty() {
            return "Noch keine Aktivität.".into();
        }
        lines.join("\n")
    }

    /// Log lines of a step of „implement review comments“.
    fn address_log(&self, lines: &mut Vec<String>, step: usize, review: u32, comments: usize) {
        let o = &self.options;
        let hint = o.hint.trim();
        match step {
            0 => {
                lines.push(format!(
                    "› Lese {} aus Review #{review}",
                    plural_comments(comments)
                ));
                if !hint.is_empty() {
                    lines.push(format!("› Hinweis: {hint}"));
                }
                if let Some(plan) = self.plan() {
                    lines.push(format!(
                        "› Plan erstellt: {} Schritte, {} Dateien",
                        plan.steps.len(),
                        plan.files.len()
                    ));
                    for note in &self.plan_notes {
                        lines.push(format!("› Neuer Plan angefordert: {note}"));
                    }
                    lines.push(if self.is_awaiting() {
                        "› Warte auf Freigabe".into()
                    } else {
                        "› Plan freigegeben".into()
                    });
                }
            }
            1 => lines.push(format!("› git switch {}", self.branch())),
            2 => lines.push(format!("› Bearbeite {}", self.files.join(", "))),
            3 if o.run_tests => lines.push("› Starte Lint und Tests … bestanden".into()),
            3 => lines.push("› Tests übersprungen".into()),
            _ => lines.push(format!(
                "› git push origin {} (kein neuer Pull Request)",
                self.branch()
            )),
        }
    }

    /// Log lines of a step of „investigate check“.
    fn diagnose_log(&self, lines: &mut Vec<String>, step: usize, review: u32, check: &str) {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        match step {
            0 => lines.push(format!("› Lese das Log von „{check}“ in #{review}")),
            1 => lines.push("› Suche Fehlermeldung, Datei und Zeile".into()),
            2 => lines.push(match &self.diagnosis {
                Some(d) => format!("› Diagnose: {}", d.cause),
                None => "› Diagnose erstellt".into(),
            }),
            3 if code => lines.push("› Wende den Fix an, starte Lint und Tests … bestanden".into()),
            3 => lines.push("› Kein Code-Fix nötig, der Branch bleibt unverändert".into()),
            _ if code => lines.push(format!(
                "› git push origin {}, starte „{check}“ neu",
                self.branch()
            )),
            _ => lines.push(format!("› Starte „{check}“ neu")),
        }
    }

    /// Log up to and including the step the agent is currently working on.
    pub fn log(&self) -> String {
        if self.real || self.ended.is_some() {
            return self.real_log();
        }
        let shown = if self.is_awaiting() {
            1
        } else if self.is_diagnosed() {
            self.completed
        } else if self.is_working() {
            self.completed + 1
        } else {
            self.completed
        }
        .min(STEPS);
        if shown == 0 {
            return "Noch keine Aktivität. Starte den Agenten, um das Ticket umzusetzen.".into();
        }
        let o = &self.options;
        let hint = o.hint.trim();
        let mut lines = Vec::new();
        for step in 0..shown {
            match (&self.kind, step) {
                (RunKind::AddressReview { review, comments }, _) => {
                    self.address_log(&mut lines, step, *review, comments.len());
                }
                (RunKind::Diagnose { review, check }, _) => {
                    self.diagnose_log(&mut lines, step, *review, check);
                }
                (RunKind::Implement, 0) => {
                    let comments = match self.comments {
                        1 => "1 Kommentar".to_string(),
                        n => format!("{n} Kommentare"),
                    };
                    lines.push(format!("› Lese {} und {comments}", self.ticket_key));
                    if !hint.is_empty() {
                        lines.push(format!("› Hinweis: {hint}"));
                    }
                    for rule in self.rules.lines() {
                        lines.push(format!("› Regel: {rule}"));
                    }
                    if let Some(plan) = self.plan() {
                        lines.push(format!(
                            "› Plan erstellt: {} Schritte, {} Dateien",
                            plan.steps.len(),
                            plan.files.len()
                        ));
                        for v in &plan.violations {
                            lines.push(format!("› Regelverstoß: {}", v.reason()));
                        }
                        for note in &self.plan_notes {
                            lines.push(format!("› Neuer Plan angefordert: {note}"));
                        }
                        lines.push(if self.is_awaiting() {
                            "› Warte auf Freigabe".into()
                        } else {
                            "› Plan freigegeben".into()
                        });
                    }
                }
                (RunKind::Implement, 1) => lines.push(format!("› git switch -c {}", self.branch())),
                (RunKind::Implement, 2) => {
                    lines.push(format!("› Bearbeite {}", self.files.join(", ")))
                }
                (RunKind::Implement, 3) if !self.rules.required_tests.is_empty() => {
                    for cmd in &self.rules.required_tests {
                        lines.push(format!("› $ {cmd} … bestanden"));
                    }
                }
                (RunKind::Implement, 3) if o.run_tests => {
                    lines.push("› Starte Lint und Tests … bestanden".into())
                }
                (RunKind::Implement, 3) => lines.push("› Tests übersprungen".into()),
                (RunKind::Implement, _) => lines.push(format!(
                    "› Pull Request {} erstellt{}",
                    self.pr_label(),
                    if o.draft_pr { " (Entwurf)" } else { "" }
                )),
            }
            // Questions, answers and acknowledged hints of this step.
            for m in self.messages.iter().filter(|m| m.step == step) {
                match m.kind {
                    MessageKind::Question => lines.push(format!("› Frage: {}", m.text)),
                    MessageKind::Answer => lines.push(format!("› Antwort: {}", m.text)),
                    MessageKind::Ack => lines.push(format!("› {}", m.text)),
                    _ => {}
                }
            }
        }
        if self.is_diagnosed() {
            lines.push("› Warte auf deine Entscheidung über den Fix".into());
        } else if self.is_asking() {
            lines.push("› Warte auf deine Antwort".into());
        } else if self.is_paused() {
            lines.push("› Pausiert".into());
        } else if let Some(reason) = self.stop_reason() {
            lines.push(format!("› Angehalten: {}", reason.label()));
        }
        lines.join("\n")
    }
}

fn plural_comments(n: usize) -> String {
    match n {
        1 => "1 Kommentar".to_string(),
        n => format!("{n} Kommentare"),
    }
}

fn files_label(n: usize) -> String {
    match n {
        1 => "1 Datei".to_string(),
        n => format!("{n} Dateien"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without plan approval: the run starts immediately.
    fn direct() -> AgentOptions {
        AgentOptions {
            plan_first: false,
            ..Default::default()
        }
    }

    /// A run without a follow-up question (the demo asks only for BAU-142).
    fn quiet() -> AgentRun {
        let mut r = run();
        r.ticket_key = "BAU-200".into();
        r
    }

    fn run() -> AgentRun {
        let mut r = AgentRun::new("BAU-142", "Toggle: Fokus-Ring fehlt im Dark Mode");
        r.comments = 1;
        r.files = vec!["components.css".into(), "tokens.css".into()];
        r
    }

    #[test]
    fn a_new_run_is_idle() {
        let r = run();
        assert!(r.is_idle());
        assert!(r.log().starts_with("Noch keine Aktivität"));
        assert!(r.steps().iter().all(|s| s.state == StepState::Pending));
        assert_eq!(r.branch(), "claude/BAU-142-toggle-fokus-ring-fehlt");
    }

    #[test]
    fn advance_only_works_while_running() {
        let mut r = run();
        assert!(!r.advance());
        assert_eq!(r.completed, 0);
    }

    #[test]
    fn exactly_one_step_is_active_while_running() {
        let mut r = quiet();
        r.start(direct(), 92);
        for done in 0..STEPS {
            let states: Vec<_> = r.steps().into_iter().map(|s| s.state).collect();
            assert_eq!(
                states.iter().filter(|s| **s == StepState::Done).count(),
                done
            );
            assert_eq!(
                states.iter().filter(|s| **s == StepState::Active).count(),
                1
            );
            assert_eq!(states[done], StepState::Active);
            assert_eq!(r.log().lines().count(), done + 1);
            let finished = r.advance();
            assert_eq!(finished, done + 1 == STEPS);
        }
        assert!(r.is_done());
        assert!(r.steps().iter().all(|s| s.state == StepState::Done));
        assert!(r.log().ends_with("› Pull Request #92 erstellt (Entwurf)"));
        assert!(!r.advance(), "a finished run stays finished");
        assert_eq!(r.completed, STEPS);
    }

    #[test]
    fn options_show_up_in_steps_and_log() {
        let mut r = quiet();
        let options = AgentOptions {
            run_tests: false,
            draft_pr: false,
            hint: " Auch den Slider prüfen ".into(),
            ..direct()
        };
        r.start(options, 7);
        while !r.advance() {}
        let steps = r.steps();
        assert_eq!(steps[3].title, "Tests übersprungen");
        assert_eq!(steps[4].text, "#7 – wartet auf dein Review");
        let log = r.log();
        assert!(log.contains("› Hinweis: Auch den Slider prüfen\n"), "{log}");
        assert!(log.contains("› Tests übersprungen"));
        assert!(log.contains("› Bearbeite components.css, tokens.css"));
        assert_eq!(
            log.lines().count(),
            STEPS + 1,
            "der Hinweis ist eine Zusatzzeile"
        );
    }

    #[test]
    fn reset_makes_the_run_startable_again() {
        let mut r = run();
        r.start(direct(), 92);
        r.advance();
        r.reset();
        assert!(r.is_idle());
        assert_eq!((r.completed, r.pr_number), (0, None));
    }

    #[test]
    fn a_plan_waits_for_approval() {
        let mut r = run();
        r.options.hint = "Auch den Slider prüfen".into();
        let options = r.options.clone();
        r.start(options, 92);
        assert!(r.is_awaiting() && r.is_active() && !r.is_running());
        assert!(!r.advance(), "no progress without approval");
        assert_eq!(r.completed, 0);
        let plan = r.plan().unwrap();
        assert_eq!(plan.files, ["components.css", "tokens.css"]);
        assert!(plan.summary.contains("Auch den Slider prüfen"));
        assert!(plan.steps.iter().any(|s| s.contains("claude/BAU-142")));
        let steps = r.steps();
        assert_eq!(steps[0].state, StepState::Active);
        assert_eq!(steps[0].title, "Plan liegt vor");
        assert!(steps[1..].iter().all(|s| s.state == StepState::Pending));
        assert!(r.log().ends_with("› Warte auf Freigabe"), "{}", r.log());
        // There is no branch in the log yet.
        assert!(!r.log().contains("git switch"));

        assert!(r.approve());
        assert!(!r.approve());
        assert!(r.log().ends_with("› Plan freigegeben"), "{}", r.log());
        assert!(r.advance() || r.completed == 1);
    }

    #[test]
    fn rejecting_resets_the_run() {
        let mut r = run();
        r.start(AgentOptions::default(), 92);
        assert!(r.revise("noch eine Anmerkung"));
        assert!(r.reject());
        assert!(r.is_idle() && r.plan().is_none());
        assert_eq!(
            (r.pr_number, r.plan_notes.len(), r.planned),
            (None, 0, false)
        );
        assert!(!r.reject());
        assert!(!r.approve());
    }

    #[test]
    fn a_note_creates_a_new_plan() {
        let mut r = run();
        r.start(AgentOptions::default(), 92);
        let before = r.plan().unwrap();
        assert!(!r.revise("   "));
        assert!(r.revise("  Bitte auch Tests für den Slider  "));
        let after = r.plan().unwrap();
        assert_ne!(before, after);
        assert_eq!(after.steps.len(), before.steps.len() + 1);
        assert!(
            after
                .steps
                .iter()
                .any(|s| s == "Berücksichtigen: Bitte auch Tests für den Slider")
        );
        assert!(r.is_awaiting(), "the new plan needs approval again");
        assert!(
            r.log()
                .contains("› Neuer Plan angefordert: Bitte auch Tests für den Slider")
        );
        // After approval it can no longer be revised.
        r.approve();
        assert!(!r.revise("zu spät"));
    }

    #[test]
    fn without_plan_first_the_agent_runs_right_away() {
        let mut r = run();
        r.start(direct(), 92);
        assert!(r.is_running() && r.plan().is_none());
        assert!(!r.approve());
        assert!(!r.log().contains("Plan"));
    }

    /// Brings BAU-142 up to the follow-up question.
    fn asking() -> AgentRun {
        let mut r = run();
        r.start(direct(), 92);
        assert_eq!(r.tick(), Tick::Stepped);
        assert_eq!(r.tick(), Tick::Stepped);
        assert_eq!(r.tick(), Tick::Asked);
        r
    }

    #[test]
    fn the_agent_asks_a_question_and_waits() {
        let mut r = asking();
        assert!(r.is_asking() && r.is_active() && !r.is_running());
        assert_eq!(r.completed, 2, "the question is not a step");
        let q = r.question.clone().unwrap();
        assert!(q.text.contains("deprecated"));
        assert_eq!(q.suggestions.len(), 3);
        assert_eq!(r.tick(), Tick::Idle, "no progress without an answer");
        assert!(!r.advance());
        assert_eq!(r.completed, 2);
        let steps = r.steps();
        assert_eq!(steps[2].title, "Frage an dich");
        assert_eq!(steps[2].state, StepState::Active);
        assert!(steps[3..].iter().all(|s| s.state == StepState::Pending));
        assert!(
            r.log().contains("› Frage: Soll der alte Stil"),
            "{}",
            r.log()
        );
        assert!(r.log().ends_with("› Warte auf deine Antwort"));
    }

    #[test]
    fn an_answer_continues_the_run_and_stays_in_the_history() {
        let mut r = asking();
        assert!(!r.answer("   "), "empty does not count");
        assert!(r.is_asking());
        assert!(r.answer("  Als deprecated markieren "));
        assert!(r.is_running() && r.question.is_none());
        assert!(!r.answer("noch einmal"), "answer only once");
        let last = r.messages.last().unwrap();
        assert_eq!(
            (last.from, last.kind, last.text.as_str()),
            (
                Speaker::User,
                MessageKind::Answer,
                "Als deprecated markieren"
            )
        );
        assert!(r.log().contains("› Antwort: Als deprecated markieren"));
        // The question comes only once; the run goes to the end.
        while !r.advance() {}
        assert!(r.is_done());
        assert_eq!(
            r.messages
                .iter()
                .filter(|m| m.kind == MessageKind::Question)
                .count(),
            1
        );
        assert!(
            r.steps()[2]
                .text
                .ends_with("Deine Antwort: Als deprecated markieren")
        );
    }

    #[test]
    fn other_tickets_ask_nothing() {
        let mut r = quiet();
        r.start(direct(), 92);
        while !r.advance() {}
        assert!(r.messages.iter().all(|m| m.kind != MessageKind::Question));
    }

    #[test]
    fn a_hint_is_taken_into_account_at_the_next_step() {
        let mut r = quiet();
        r.start(direct(), 92);
        assert!(!r.steer("  "), "empty does not count");
        assert!(r.steer(" Auch den Slider prüfen "));
        assert_eq!(r.pending_hints, ["Auch den Slider prüfen"]);
        assert!(
            !r.messages.iter().any(|m| m.kind == MessageKind::Ack),
            "noch nicht berücksichtigt"
        );
        assert_eq!(r.tick(), Tick::Stepped);
        assert!(r.pending_hints.is_empty());
        let ack = r
            .messages
            .iter()
            .find(|m| m.kind == MessageKind::Ack)
            .unwrap();
        assert_eq!(
            (ack.from, ack.text.as_str()),
            (Speaker::Agent, "Hinweis erhalten: Auch den Slider prüfen")
        );
        assert!(
            r.log()
                .contains("› Hinweis erhalten: Auch den Slider prüfen")
        );
    }

    #[test]
    fn hints_only_work_while_the_agent_works() {
        let mut r = quiet();
        assert!(!r.steer("too early"), "idle");
        r.start(AgentOptions::default(), 92);
        assert!(
            !r.steer("Plan noch offen"),
            "dafür gibt es die Plan-Anmerkung"
        );
        r.approve();
        assert!(r.steer("jetzt"));
        while !r.advance() {}
        assert!(!r.steer("zu spät"), "done");
    }

    #[test]
    fn pause_and_resume() {
        let mut r = quiet();
        assert!(!r.pause() && !r.resume());
        r.start(direct(), 92);
        r.advance();
        assert!(r.pause() && r.is_paused() && r.is_active());
        assert!(!r.pause(), "schon pausiert");
        assert_eq!(r.tick(), Tick::Idle);
        assert_eq!(r.completed, 1, "paused means: no progress");
        assert_eq!(r.steps()[1].state, StepState::Active);
        assert!(r.steps()[1].text.starts_with("Pausiert"));
        assert!(r.log().ends_with("› Pausiert"));
        // Hints can also be sent while paused and wait until resume.
        assert!(r.steer("Während der Pause"));
        assert_eq!(r.tick(), Tick::Idle);
        assert_eq!(r.pending_hints.len(), 1);
        assert!(r.resume() && r.is_running() && !r.resume());
        assert_eq!(r.tick(), Tick::Stepped);
        assert!(r.pending_hints.is_empty());
    }

    #[test]
    fn a_question_cannot_be_skipped_by_pausing() {
        let mut r = asking();
        assert!(!r.pause(), "whoever asks is not running");
        assert!(r.is_asking());
    }

    #[test]
    fn reset_clears_the_conversation() {
        let mut r = asking();
        r.steer("noch ein Hinweis");
        r.reset();
        assert!(r.messages.is_empty() && r.question.is_none() && r.pending_hints.is_empty());
        // A new run asks again.
        r.start(direct(), 93);
        assert_eq!(r.messages.len(), 1);
        r.advance();
        r.advance();
        assert_eq!(r.tick(), Tick::Asked);
    }

    #[test]
    fn the_history_tells_the_story() {
        let mut r = run();
        r.start(
            AgentOptions {
                hint: "Bitte vorsichtig".into(),
                ..AgentOptions::default()
            },
            92,
        );
        r.revise("Auch Tests");
        r.approve();
        let texts: Vec<_> = r.messages.iter().map(|m| (m.from, m.kind)).collect();
        assert_eq!(
            texts,
            [
                (Speaker::Agent, MessageKind::Info),
                (Speaker::User, MessageKind::Hint),
                (Speaker::Agent, MessageKind::Info),
                (Speaker::User, MessageKind::Hint),
                (Speaker::User, MessageKind::Info),
            ]
        );
    }

    fn real_run() -> AgentRun {
        let mut r = run();
        r.start(
            AgentOptions {
                plan_first: false,
                ..AgentOptions::default()
            },
            92,
        );
        r.report(
            AgentReport::Started {
                detail: "Container gestartet".into(),
            },
            &AgentLimits::unlimited(),
            0,
        );
        r
    }

    fn step(tokens_in: u64, tokens_out: u64, millis: u64) -> AgentReport {
        AgentReport::Step {
            title: "Datei lesen".into(),
            text: "src/lib.rs".into(),
            input_tokens: tokens_in,
            output_tokens: tokens_out,
            millis,
        }
    }

    #[test]
    fn a_real_run_is_not_touched_by_the_simulated_tick() {
        let mut r = real_run();
        assert!(r.real);
        assert_eq!(r.tick(), Tick::Idle);
        assert_eq!(r.completed, 0);
    }

    #[test]
    fn steps_record_usage_with_the_prices_and_never_finish_the_run() {
        let mut r = real_run();
        let limits = AgentLimits::unlimited();
        for _ in 0..10 {
            let e = r.report(step(1_000_000, 0, 1_000), &limits, 0).unwrap();
            assert_eq!(e.usage.unwrap().cost_cents, 300);
            assert!(!e.finished);
        }
        assert_eq!(r.completed, STEPS - 1);
        assert!(r.is_running());
        assert_eq!(r.usage().cost_cents, 3_000);
        assert_eq!(r.usage().millis, 10_000);
    }

    #[test]
    fn the_budget_stops_a_real_run_with_a_plain_message() {
        let mut r = real_run();
        let limits = AgentLimits {
            run_budget_cents: Some(100),
            ..AgentLimits::unlimited()
        };
        let e = r.report(step(1_000_000, 0, 10), &limits, 0).unwrap();
        assert_eq!(e.stopped, Some(StopReason::RunBudget));
        assert_eq!(r.stop_reason(), Some(StopReason::RunBudget));
        assert!(
            r.messages
                .last()
                .unwrap()
                .text
                .starts_with("Angehalten: Budget pro Lauf erreicht")
        );
    }

    #[test]
    fn the_month_budget_counts_what_the_step_just_cost() {
        let mut r = real_run();
        let limits = AgentLimits {
            month_budget_cents: Some(400),
            ..AgentLimits::unlimited()
        };
        let e = r.report(step(1_000_000, 0, 10), &limits, 150).unwrap();
        assert_eq!(e.stopped, Some(StopReason::MonthBudget));
    }

    #[test]
    fn a_question_waits_for_the_answer_and_the_answer_resumes() {
        let mut r = real_run();
        let limits = AgentLimits::unlimited();
        let e = r
            .report(
                AgentReport::Question {
                    text: "Welcher Branch?".into(),
                    suggestions: vec!["main".into()],
                },
                &limits,
                0,
            )
            .unwrap();
        assert!(e.asking && r.is_asking());
        assert_eq!(r.question.as_ref().unwrap().suggestions, ["main"]);
        assert!(r.answer("main"));
        assert!(r.is_running());
    }

    #[test]
    fn hints_are_acknowledged_when_the_container_got_them() {
        let mut r = real_run();
        r.steer("Bitte kleine Commits");
        assert_eq!(r.pending_hints.len(), 1);
        r.report(
            AgentReport::HintsDelivered { count: 1 },
            &AgentLimits::unlimited(),
            0,
        );
        assert!(r.pending_hints.is_empty());
        assert_eq!(r.messages.last().unwrap().kind, MessageKind::Ack);
    }

    #[test]
    fn done_finishes_and_failed_resets_with_the_reason() {
        let mut r = real_run();
        let limits = AgentLimits::unlimited();
        let e = r
            .report(
                AgentReport::Done {
                    summary: "Fertig".into(),
                },
                &limits,
                0,
            )
            .unwrap();
        assert!(e.finished && r.is_done() && r.completed == STEPS);

        let mut r = real_run();
        let e = r
            .report(
                AgentReport::Failed {
                    reason: "Der Container endete mit Code 1".into(),
                },
                &limits,
                0,
            )
            .unwrap();
        assert!(e.failed && r.is_idle() && !r.real);
        assert_eq!(r.ended.as_deref(), Some("Der Container endete mit Code 1"));
        assert!(r.log().contains("Der Container endete mit Code 1"));
        // A new start clears the reason.
        r.start(AgentOptions::default(), 92);
        assert_eq!(r.ended, None);
    }

    #[test]
    fn reports_for_an_inactive_run_change_nothing() {
        let mut r = run();
        let before = r.clone();
        assert!(
            r.report(step(1, 1, 1), &AgentLimits::unlimited(), 0)
                .is_none()
        );
        assert_eq!(r, before);
    }

    #[test]
    fn the_plan_comes_from_the_agent_and_cannot_be_approved_before() {
        let mut r = run();
        let limits = AgentLimits::unlimited();
        r.start(AgentOptions::default(), 92);
        r.report(
            AgentReport::Started {
                detail: "Plan-Lauf".into(),
            },
            &limits,
            0,
        );
        assert!(r.is_drafting());
        assert!(!r.approve(), "without a plan there is nothing to approve");
        r.report(
            AgentReport::Plan {
                summary: "Fokus-Ring ergänzen".into(),
                files: vec!["src/toggle.rs".into()],
                steps: vec!["Lesen".into(), "Ändern".into()],
            },
            &limits,
            0,
        );
        let plan = r.plan().unwrap();
        assert_eq!(plan.summary, "Fokus-Ring ergänzen");
        assert_eq!(r.files, ["src/toggle.rs"]);
        assert!(!r.is_drafting());
        // A note demands a new plan.
        assert!(r.revise("Auch Tests"));
        assert!(r.is_drafting() && !r.approve());
        r.report(
            AgentReport::Plan {
                summary: "Neu".into(),
                files: vec![],
                steps: vec![],
            },
            &limits,
            0,
        );
        assert!(r.approve());
    }

    #[test]
    fn a_plan_report_outside_the_approval_is_ignored() {
        let mut r = real_run();
        assert!(
            r.report(
                AgentReport::Plan {
                    summary: "x".into(),
                    files: vec![],
                    steps: vec![]
                },
                &AgentLimits::unlimited(),
                0
            )
            .is_none()
        );
    }
}
