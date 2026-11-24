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
    /// Language of the pull request title and text. The backend sets it from the repo's
    /// configuration at the start.
    #[serde(skip_serializing_if = "crate::repos::is_english")]
    pub pr_language: cockpit_i18n::Lang,
    /// Only for a review run: the language of the drafted comments and why it was chosen. Set when
    /// the run starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_language: Option<crate::language::LangChoice>,
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
    /// All repos of the run (the first is `repo`). Empty = only `repo`. The interface sends the
    /// choice here; the backend settles it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
    /// Base branch, pattern and language per repo, set by the backend from the configuration
    /// (whatever a request carries is replaced).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RepoTarget>,
}

/// The settings of one repo of a start, from the configuration.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoTarget {
    pub repo: String,
    #[serde(default)]
    pub base_branch: String,
    #[serde(default)]
    pub branch_pattern: String,
    #[serde(default)]
    pub pr_language: cockpit_i18n::Lang,
}

impl AgentOptions {
    /// The repos chosen for the run, in order, without duplicates and empty names. Old requests
    /// that only know `repo` give that one.
    pub fn chosen_repos(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let all = self.repos.iter().cloned().chain(self.repo.clone());
        for r in all {
            let r = r.trim().to_string();
            if !r.is_empty() && !out.contains(&r) {
                out.push(r);
            }
        }
        out.truncate(super::MAX_REPOS_PER_RUN);
        out
    }
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            run_tests: true,
            draft_pr: true,
            hint: String::new(),
            base_branch: "main".into(),
            branch_pattern: crate::branch::DEFAULT_PATTERN.into(),
            pr_language: cockpit_i18n::Lang::En,
            review_language: None,
            branch_type: crate::branch::BranchType::Feature.as_str().into(),
            plan_first: true,
            repo: None,
            repos: Vec::new(),
            targets: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunState {
    /// Ready to start, or ended (cancelled, failed). A stored `AwaitingFix` (the diagnosis of a
    /// follow-up job that no longer exists) reads as this.
    #[default]
    #[serde(alias = "AwaitingFix")]
    Idle,
    /// The plan is ready, the agent waits for the user.
    AwaitingApproval,
    Running,
    /// The agent asked a question and waits for the answer.
    AwaitingAnswer,
    /// The user has paused the run.
    Paused,
    /// A limit (budget, time limit) is reached: the run waits for you.
    Stopped(StopReason),
    Done,
}

/// How the pull request of a run ended. A run whose pull request is merged or closed is „erledigt“.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closure {
    Merged,
    Closed,
}

impl Closure {
    /// The state as the interface shows it.
    pub fn label_msg(self) -> cockpit_i18n::Msg {
        match self {
            Closure::Merged => cockpit_i18n::msg!(CoreAgentClosureMerged),
            Closure::Closed => cockpit_i18n::msg!(CoreAgentClosureClosed),
        }
    }

    /// [`Closure::label_msg`] in German (plain-text readers, tests).
    pub fn label(self) -> String {
        self.label_msg().de()
    }
}

/// What the run does.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(from = "RunKindStored")]
pub enum RunKind {
    /// Implement a ticket and create a pull request.
    #[default]
    Implement,
    /// Follow-up job on the branch of an existing pull request: the
    /// backend reads the inputs (review threads, failed checks) from GitHub when the run starts and
    /// hands them to the container. The agent continues on the same `claude/…` branch; the backend
    /// pushes the new commits (never a force-push).
    Followup {
        /// The pull request (repo and number).
        review: PrKey,
        task: FollowupTask,
    },
    /// The agent reads a pull request and prepares review comments as drafts. It only
    /// reads: the clone of the branch is mounted read-only, nothing is pushed, nothing goes to
    /// GitHub. The drafts become suggestions of the review and wait for your decision.
    ReviewPr {
        /// The pull request (repo and number).
        review: PrKey,
    },
}

/// How a stored run kind is read: the current kinds, plus the two kinds of the simulated agent
/// that older states may still hold. They become the follow-up job with the same purpose.
#[derive(Deserialize)]
enum RunKindStored {
    Implement,
    AddressReview { review: PrKey },
    Diagnose { review: PrKey, check: String },
    Followup { review: PrKey, task: FollowupTask },
    ReviewPr { review: PrKey },
}

impl From<RunKindStored> for RunKind {
    fn from(stored: RunKindStored) -> Self {
        match stored {
            RunKindStored::Implement => RunKind::Implement,
            RunKindStored::AddressReview { review } => RunKind::Followup {
                review,
                task: FollowupTask::ReviewThreads,
            },
            RunKindStored::Diagnose { review, check } => RunKind::Followup {
                review,
                task: FollowupTask::FailedChecks {
                    checks: vec![check],
                },
            },
            RunKindStored::Followup { review, task } => RunKind::Followup { review, task },
            RunKindStored::ReviewPr { review } => RunKind::ReviewPr { review },
        }
    }
}

/// What a follow-up job does.
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
            RunKind::Followup { review, .. } | RunKind::ReviewPr { review } => Some(review.clone()),
        }
    }
}
