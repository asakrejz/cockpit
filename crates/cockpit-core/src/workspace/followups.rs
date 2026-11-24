use cockpit_i18n::msg;

use super::*;
use crate::review::AgentComment;

impl Workspace {
    // ---------- Follow-up jobs: implement review comments, fix red checks, review a pull request ----------

    /// The key of the entry the pull request hangs on (ticket or PR entry).
    pub(super) fn item_key_of_pr(&self, key: &PrKey) -> String {
        self.items
            .iter()
            .find(|i| {
                i.pr.as_ref()
                    .is_some_and(|p| key.matches(&p.repo, p.number))
            })
            .map_or_else(
                || key.to_string(),
                |i| match &i.pr {
                    // A pull request without ticket is named with its repo.
                    Some(p) if i.key.starts_with('#') && !p.repo.is_empty() => p.key().to_string(),
                    _ => i.key.clone(),
                },
            )
    }

    /// Is a follow-up job already running or waiting on this pull request?
    pub fn active_followup(&self, review: impl Into<PrKey>) -> Option<&AgentRun> {
        let review = review.into();
        self.runs.iter().find(|r| {
            r.is_followup() && r.kind.review().is_some_and(|k| review.same(&k)) && r.is_active()
        })
    }

    /// All follow-up jobs of a pull request, also the finished ones.
    pub fn followups(&self, review: impl Into<PrKey>) -> Vec<&AgentRun> {
        let review = review.into();
        self.runs
            .iter()
            .filter(|r| r.is_followup() && r.kind.review().is_some_and(|k| review.same(&k)))
            .collect()
    }

    /// Checks that a follow-up job may start on the pull request.
    pub(super) fn check_followup(&self, review: &PrKey) -> Result<&Review, AgentError> {
        let r = self.review(review).ok_or(AgentError::UnknownReview)?;
        if r.merged.is_some() {
            return Err(AgentError::Merged);
        }
        if self.active_followup(r.key()).is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        Ok(r)
    }

    pub(super) fn start_followup(&mut self, mut run: AgentRun, options: AgentOptions) -> String {
        run.start(options);
        let key = run.key();
        let waiting = run.is_awaiting();
        let note = match &run.kind {
            _ if waiting => msg!(CoreWorkspaceAgentWaitingPlan),
            RunKind::Followup {
                task: FollowupTask::FailedChecks { .. },
                ..
            } => msg!(CoreWorkspaceAgentFixingChecks),
            RunKind::ReviewPr { .. } => msg!(CoreWorkspaceAgentReviewing),
            _ => msg!(CoreWorkspaceAgentAddressing),
        };
        let pr = run.pr_key();
        match self.runs.iter_mut().find(|r| r.key() == key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = pr.and_then(|k| self.pr_item_mut(k)) {
            item.set_note(note);
        }
        key
    }

    /// Commissions the agent to implement the review comments of the pull request. The
    /// backend reads the threads from GitHub when the run starts. The agent only continues on
    /// branches that the repo's pattern names (`claude/…` by default). Returns the key of the run (`threads-91`).
    pub fn address_review_threads(
        &mut self,
        review: impl Into<PrKey>,
        options: AgentOptions,
    ) -> Result<String, AgentError> {
        let review = review.into();
        let r = self.check_followup(&review)?;
        let review = r.key();
        let (title, branch) = (r.title.clone(), r.branch.clone());
        if crate::branch::check_existing(&options.branch_pattern, &options.base_branch, &branch)
            .is_err()
        {
            return Err(AgentError::NotAgentBranch);
        }
        let run = AgentRun::followup(
            RunKind::Followup {
                review: review.clone(),
                task: FollowupTask::ReviewThreads,
            },
            &self.item_key_of_pr(&review),
            &title,
            &branch,
        );
        Ok(self.start_followup(run, options))
    }

    /// Commissions the agent to fix the red checks of the pull request. Returns the key
    /// of the run (`fixes-91`).
    pub fn fix_failed_checks(
        &mut self,
        review: impl Into<PrKey>,
        options: AgentOptions,
    ) -> Result<String, AgentError> {
        let review = review.into();
        let r = self.check_followup(&review)?;
        let review = r.key();
        let checks: Vec<String> = r
            .check_runs
            .iter()
            .filter(|c| c.status == Checks::Failed)
            .map(|c| c.name.clone())
            .collect();
        if checks.is_empty() {
            return Err(AgentError::CheckNotFailed);
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        if crate::branch::check_existing(&options.branch_pattern, &options.base_branch, &branch)
            .is_err()
        {
            return Err(AgentError::NotAgentBranch);
        }
        let run = AgentRun::followup(
            RunKind::Followup {
                review: review.clone(),
                task: FollowupTask::FailedChecks { checks },
            },
            &self.item_key_of_pr(&review),
            &title,
            &branch,
        );
        Ok(self.start_followup(run, options))
    }

    /// Commissions the agent to read the pull request and prepare review comments as
    /// drafts. The run only reads: no push, no token, nothing goes to GitHub. Works on every
    /// branch. The old suggestions are replaced. Returns the key of the run (`prreview-…`).
    ///
    /// The language of the drafts: `chosen` (the user's switch), else detected from the pull request
    /// (and the ticket if the description is short), else `repo_language`, else English.
    pub fn review_pr_with_agent(
        &mut self,
        review: impl Into<PrKey>,
        chosen: Option<cockpit_i18n::Lang>,
        repo_language: Option<cockpit_i18n::Lang>,
    ) -> Result<String, AgentError> {
        let review = review.into();
        let r = self.check_followup(&review)?;
        if r.files.is_empty() {
            return Err(AgentError::NothingToReview);
        }
        let review = r.key();
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let ticket_key = self.item_key_of_pr(&review);
        let ticket = self
            .item_by_key(&ticket_key)
            .map_or("", |i| i.description.as_str());
        let language =
            crate::language::choose_review(chosen, &r.title, &r.description, ticket, repo_language);
        let run = AgentRun::followup(
            RunKind::ReviewPr {
                review: review.clone(),
            },
            &self.item_key_of_pr(&review),
            &title,
            &branch,
        );
        // Reading needs no plan and no tests: your click is the approval.
        let options = AgentOptions {
            plan_first: false,
            run_tests: false,
            review_language: Some(language),
            ..AgentOptions::default()
        };
        if let Some(r) = self.review_mut(&review) {
            r.begin_agent_review();
        }
        Ok(self.start_followup(run, options))
    }

    /// Books a draft or the summary of a review run on the review. `false` if the run is not a
    /// running review run or the draft does not fit the diff.
    pub(super) fn book_review_report(&mut self, key: &str, report: &AgentReport) -> bool {
        let Some(review) = self
            .runs
            .iter()
            .find(|r| r.key() == key && r.is_running())
            .and_then(|r| match &r.kind {
                RunKind::ReviewPr { review } => Some(review.clone()),
                _ => None,
            })
        else {
            return false;
        };
        let Some(r) = self.review_mut(&review) else {
            return false;
        };
        match report {
            AgentReport::ReviewComment {
                path,
                line,
                start_line,
                side,
                text,
                suggestion,
            } => r.add_agent_comment(AgentComment {
                path: path.clone(),
                line: *line,
                start_line: *start_line,
                side: *side,
                text: text.clone(),
                suggestion: suggestion.clone(),
            }),
            AgentReport::ReviewSummary { text } => {
                r.set_agent_summary(text);
                true
            }
            _ => false,
        }
    }

    /// The review run is done: the drafts wait for your decision.
    pub(super) fn finish_review_run(&mut self, run: &AgentRun) {
        let Some(key) = run.kind.review() else { return };
        let count = self.review_mut(&key).map_or(0, Review::finish_agent_review);
        if let Some(item) = self.pr_item_mut(&key) {
            item.set_note(drafts_note(count));
            item.time = "gerade eben".into();
            item.read = false;
        }
    }
}

/// The note after an agent review: nothing to object to, or how many drafts wait for approval.
pub(super) fn drafts_note(count: usize) -> cockpit_i18n::Msg {
    match count {
        0 => msg!(CoreWorkspaceAgentFoundNothing),
        n => msg!(CoreWorkspaceAgentDraftsWaiting, n = n),
    }
}
