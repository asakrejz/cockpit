//! The data that belong to a run: report, messages, plan and steps.

use super::*;

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
