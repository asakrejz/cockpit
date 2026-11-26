//! The data that belong to a run: report, messages, plan and steps.

use super::*;
use crate::review::DiffSide;
use cockpit_i18n::{Lang, Msg};

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

/// The plan the agent presented from the container (instead of the derived one).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentPlan {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
}

/// What the agent reports from the container. A report reaches the run as
/// `Action::ReportAgent`; the agent can change the state only through these paths.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AgentReport {
    /// The container is running. `detail` is a sentence for the history (runtime, mode).
    Started { detail: Msg },
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
    /// A draft review comment (only in a review run). The workspace checks it against the diff
    /// of the pull request and drops it if it does not fit.
    ReviewComment {
        path: String,
        line: u32,
        #[serde(default)]
        start_line: Option<u32>,
        side: DiffSide,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    /// The agent's summary of the pull request (only in a review run); stays local.
    ReviewSummary { text: String },
    /// What became of each repo of the run (unchanged, pushed, refused). Sent right before
    /// `Done` (or `Failed`); one refused repo does not stop the others.
    Repos { repos: Vec<RepoResult> },
    /// Done: the branch is pushed.
    Done { summary: Msg },
    /// The run ended without result (error, timeout, restart).
    Failed { reason: Msg },
}

/// What a report of the agent has effected; the workspace books and logs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportEffect {
    /// Sentences for the log, as code and parameters.
    pub notes: Vec<Msg>,
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
    /// The words the user or the agent wrote; German text of an old message of the system.
    /// Empty for a message of the system, which carries `msg`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The message as code and parameters, for texts the system wrote. Shown in the language of
    /// the interface; without it, or with a code this build no longer knows, `text` is shown as
    /// it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<Msg>,
}

impl RunMessage {
    /// The text in `lang`.
    pub fn text_in(&self, lang: Lang) -> String {
        self.msg
            .as_ref()
            .map_or_else(|| self.text.clone(), |m| m.render_or(lang, &self.text))
    }
}

/// What the agent intends to do. Derived from ticket, hints and files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub summary: Msg,
    pub files: Vec<String>,
    pub steps: Vec<Msg>,
    /// The rules of the repo that apply to this run (one line per rule).
    pub rules: Vec<Msg>,
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
    pub title: Msg,
    pub text: Msg,
    pub state: StepState,
    /// Consumption of the step once it is completed.
    pub usage: Option<Usage>,
}

impl Step {
    pub(super) fn new(title: Msg, text: Msg, state: StepState, usage: Option<Usage>) -> Self {
        Self {
            title,
            text,
            state,
            usage,
        }
    }
}

impl Plan {
    pub(super) fn new(
        summary: Msg,
        files: Vec<String>,
        steps: Vec<Msg>,
        rules: Vec<Msg>,
        violations: Vec<PathViolation>,
    ) -> Self {
        Self {
            summary,
            files,
            steps,
            rules,
            violations,
        }
    }
}
