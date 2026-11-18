//! Options of a run and its state and kind.

use super::*;

// Default values are not written, so saved states stay as they were.
fn is_default_pattern(p: &String) -> bool {
    p == crate::branch::DEFAULT_PATTERN
}

fn is_feature(t: &String) -> bool {
    t == crate::branch::BranchType::Feature.as_str()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct AgentOptions {
    pub run_tests: bool,
    /// Create the pull request as a draft.
    pub draft_pr: bool,
    /// Additional hints to the agent.
    pub hint: String,
    pub base_branch: String,
    /// How the branch is named (see [`crate::branch`]). The backend sets it from the repo's
    /// configuration at the start; whatever a request carries is replaced.
    #[serde(skip_serializing_if = "is_default_pattern")]
    pub branch_pattern: String,
    /// The `{type}` of the branch: `feature`, `fix` or `chore`. Set by the backend from the
    /// Jira issue type.
    #[serde(skip_serializing_if = "is_feature")]
    pub branch_type: String,
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
            branch_pattern: crate::branch::DEFAULT_PATTERN.into(),
            branch_type: crate::branch::BranchType::Feature.as_str().into(),
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
    /// The **real** agent reads a pull request and prepares review comments as drafts. It only
    /// reads: the clone of the branch is mounted read-only, nothing is pushed, nothing goes to
    /// GitHub. The drafts become suggestions of the review and wait for your decision.
    ReviewPr {
        /// The pull request (repo and number).
        review: PrKey,
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
            | RunKind::Followup { review, .. }
            | RunKind::ReviewPr { review } => Some(review.clone()),
        }
    }

    /// Does the real agent run this job in a container (as opposed to the simulation)?
    pub fn runs_in_container(&self) -> bool {
        matches!(
            self,
            RunKind::Implement | RunKind::Followup { .. } | RunKind::ReviewPr { .. }
        )
    }
}
