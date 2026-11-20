use cockpit_i18n::{ArgValue, Msg, msg};

use super::*;

/// Why an action was not applied. The workspace then stays unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    UnknownItem(String),
    UnknownReview(PrKey),
    UnknownSuggestion(String),
    UnknownComment(String),
    /// No ticket with this key.
    UnknownTicket(String),
    /// Text must not be empty, or the file and line do not exist.
    InvalidInput,
    /// Pull requests have no status.
    NotATicket,
    BranchExists,
    /// Nothing changes (already done, same status …).
    Unchanged,
    /// Nothing is running that would allow it (no waiting plan, no active run, no agent).
    NothingToDo,
    Send(SendError),
    Agent(AgentError),
    /// No check with this name.
    UnknownCheck(String),
    /// The pull request is merged and can no longer be changed.
    Merged(PrKey),
    /// This person does not exist in the team.
    UnknownPerson(String),
    /// Merging happens only by hand: neither the agent nor the system may do it.
    MergeNotAllowed(Actor),
    /// As many runs are already working as the limit allows (the number).
    ParallelLimit(u32),
    /// The plan touches files that the rules of the repo do not allow.
    PlanViolatesRules(Vec<PathViolation>),
    /// Even with the raise the limit is still reached.
    LimitStillReached(StopReason),
    /// This repo does not exist.
    UnknownRepo(String),
    /// Only the system triggers this action (the sync).
    SystemOnly,
    /// There are still reasons against merging.
    MergeBlocked(Vec<MergeBlocker>),
    /// This action comes only from the agent in the container.
    AgentOnly,
    /// Only you may trigger this (not the agent, not the system).
    UserOnly(Actor),
    /// The run has no pushed branch yet that could get a pull request.
    PullRequestNotReady,
    /// The run already has its pull request (number).
    PullRequestExists(u32),
    /// This only works for issues of a GitHub project (not for drafts, Jira tickets or pull requests).
    NotAnIssue,
    /// This only works for drafts of a GitHub project.
    NotADraft,
    /// The change needs the backend: it happens at GitHub first.
    NeedsBackend,
    /// The project has no such field, or the field has no such choice.
    UnknownField(String),
    /// The run still works or waits (its label); it has to be cancelled before it can be removed.
    RunStillActive(String),
}

impl ActionError {
    /// Explanation for the UI: code and parameters, the UI renders it in its language.
    pub fn msg(&self) -> Msg {
        use super::labels;
        match self {
            ActionError::UnknownItem(id) => msg!(CoreActionErrUnknownItem, id = id),
            ActionError::UnknownReview(n) => {
                msg!(CoreActionErrUnknownReview, review = n.to_string())
            }
            ActionError::UnknownSuggestion(id) => msg!(CoreActionErrUnknownSuggestion, id = id),
            ActionError::UnknownComment(id) => msg!(CoreActionErrUnknownComment, id = id),
            ActionError::UnknownTicket(key) => msg!(CoreActionErrUnknownTicket, key = key),
            ActionError::InvalidInput => msg!(CoreActionErrInvalidInput),
            ActionError::NotATicket => msg!(CoreActionErrNotATicket),
            ActionError::BranchExists => msg!(CoreActionErrBranchExists),
            ActionError::Unchanged => msg!(CoreActionErrUnchanged),
            ActionError::NothingToDo => msg!(CoreActionErrNothingToDo),
            ActionError::Send(SendError::Undecided(n)) => msg!(CoreActionErrUndecided, n = *n),
            ActionError::Send(SendError::AlreadySent) => msg!(CoreActionErrAlreadySent),
            ActionError::Send(SendError::UnknownReview) => msg!(CoreActionErrNoReview),
            ActionError::Agent(AgentError::UnknownTicket) => msg!(CoreActionErrNoTicket),
            ActionError::Agent(AgentError::AlreadyRunning) => msg!(CoreActionErrAlreadyRunning),
            ActionError::Agent(AgentError::UnknownReview) => msg!(CoreActionErrNoReview),
            ActionError::Agent(AgentError::Merged) => msg!(CoreActionErrPrMerged),
            ActionError::Agent(AgentError::NoComments) => msg!(CoreActionErrNoComments),
            ActionError::Agent(AgentError::NotAgentBranch) => msg!(CoreActionErrNotAgentBranch),
            ActionError::Agent(AgentError::NothingToReview) => {
                msg!(CoreActionErrNothingToReview)
            }
            ActionError::Agent(AgentError::CheckNotFailed) => msg!(CoreActionErrCheckNotFailed),
            ActionError::Agent(AgentError::ParallelLimit(n)) | ActionError::ParallelLimit(n) => {
                msg!(CoreActionErrParallelLimit, n = *n)
            }
            ActionError::PlanViolatesRules(violations) => msg!(
                CoreActionErrPlanViolates,
                reasons = ArgValue::joined(violations.iter().map(labels::violation), "; ")
            ),
            ActionError::LimitStillReached(reason) => {
                msg!(
                    CoreActionErrLimitStillReached,
                    reason = labels::stop(*reason)
                )
            }
            ActionError::UnknownRepo(repo) => msg!(CoreActionErrUnknownRepo, repo = repo),
            ActionError::UnknownCheck(name) => msg!(CoreActionErrUnknownCheck, check = name),
            ActionError::UnknownPerson(name) => msg!(CoreActionErrUnknownPerson, name = name),
            ActionError::Merged(n) => msg!(CoreActionErrMerged, review = n.to_string()),
            ActionError::MergeNotAllowed(actor) => {
                msg!(CoreActionErrMergeNotAllowed, who = labels::who(*actor))
            }
            ActionError::SystemOnly => msg!(CoreActionErrSystemOnly),
            ActionError::AgentOnly => msg!(CoreActionErrAgentOnly),
            ActionError::UserOnly(actor) => {
                msg!(CoreActionErrUserOnly, who = labels::who(*actor))
            }
            ActionError::NotAnIssue => msg!(CoreActionErrNotAnIssue),
            ActionError::NotADraft => msg!(CoreActionErrNotADraft),
            ActionError::RunStillActive(label) => {
                msg!(CoreActionErrRunStillActive, label = label)
            }
            ActionError::NeedsBackend => msg!(CoreActionErrNeedsBackend),
            ActionError::UnknownField(f) => msg!(CoreActionErrUnknownField, field = f),
            ActionError::PullRequestNotReady => msg!(CoreActionErrPrNotReady),
            ActionError::PullRequestExists(n) => msg!(CoreActionErrPrExists, n = *n),
            ActionError::MergeBlocked(blockers) => msg!(
                CoreActionErrMergeBlocked,
                reasons = ArgValue::joined(blockers.iter().map(|b| labels::blocker(*b)), " ")
            ),
        }
    }

    /// The German text, for readers without a language (logs, tests).
    pub fn message(&self) -> String {
        self.msg().de()
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ActionError {}

/// Error when commissioning: the limit of parallel runs has its own message.
pub(super) fn agent_error(e: AgentError) -> ActionError {
    match e {
        AgentError::ParallelLimit(n) => ActionError::ParallelLimit(n),
        other => ActionError::Agent(other),
    }
}
