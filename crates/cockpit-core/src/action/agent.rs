//! Handlers of the agent actions: starting, plan, answers, reports, follow-up jobs, pull request
//! creation and runs (one `perform_<variant>` per [`Action`]).

use cockpit_i18n::{Msg, msg};

use super::*;

impl Workspace {
    pub(super) fn perform_start_agent(
        &mut self,
        ticket: String,
        options: AgentOptions,
    ) -> Result<Vec<Note>, ActionError> {
        self.start_agent(&ticket, options).map_err(agent_error)?;
        // The rules of the repo can force a plan, even if none was requested.
        let plan_first = self
            .runs
            .iter()
            .any(|r| r.key() == ticket && r.is_awaiting());
        let mut notes = vec![Note::coded(
            Target::Run(ticket.clone()),
            msg!(CoreActionLogAgentStarted, ticket = &ticket),
        )];
        if plan_first {
            notes.push(
                Note::coded(
                    Target::Run(ticket.clone()),
                    msg!(CoreActionLogPlanPresented, ticket = &ticket),
                )
                .by(Actor::Agent),
            );
        }
        Ok(notes)
    }

    pub(super) fn perform_cancel_agent(
        &mut self,
        ticket: String,
    ) -> Result<Vec<Note>, ActionError> {
        let Some(label) = self
            .runs
            .iter()
            .find(|r| r.key() == ticket && r.is_active())
            .map(labels::run_label)
        else {
            return Err(ActionError::NothingToDo);
        };
        self.cancel_agent(&ticket);
        Ok(vec![Note::coded(
            Target::Run(ticket),
            msg!(CoreActionLogRunCancelled, label = label),
        )])
    }

    pub(super) fn perform_approve_plan(
        &mut self,
        ticket: String,
    ) -> Result<Vec<Note>, ActionError> {
        let label = self.label_of(&ticket);
        if let Some(run) = self
            .runs
            .iter()
            .find(|r| r.key() == ticket && r.is_awaiting())
        {
            let violations = run.violations();
            if !violations.is_empty() {
                return Err(ActionError::PlanViolatesRules(violations));
            }
            if let Some(max) = self.parallel_full() {
                return Err(ActionError::ParallelLimit(max));
            }
        }
        if !self.approve_plan(&ticket) {
            return Err(ActionError::NothingToDo);
        }
        Ok(vec![Note::coded(
            Target::Run(ticket),
            msg!(CoreActionLogPlanApproved, label = label),
        )])
    }

    pub(super) fn perform_reject_plan(&mut self, ticket: String) -> Result<Vec<Note>, ActionError> {
        let label = self.label_of(&ticket);
        if !self.reject_plan(&ticket) {
            return Err(ActionError::NothingToDo);
        }
        Ok(vec![Note::coded(
            Target::Run(ticket),
            msg!(CoreActionLogPlanRejected, label = label),
        )])
    }

    pub(super) fn perform_revise_plan(
        &mut self,
        ticket: String,
        note: String,
    ) -> Result<Vec<Note>, ActionError> {
        let waiting = self
            .runs
            .iter()
            .any(|r| r.key() == ticket && r.is_awaiting());
        if !waiting {
            return Err(ActionError::NothingToDo);
        }
        let label = self.label_of(&ticket);
        if !self.revise_plan(&ticket, &note) {
            return Err(ActionError::InvalidInput);
        }
        Ok(vec![Note::coded(
            Target::Run(ticket),
            msg!(CoreActionLogPlanRevised, label = label),
        )])
    }

    pub(super) fn perform_answer_agent(
        &mut self,
        run: String,
        text: String,
    ) -> Result<Vec<Note>, ActionError> {
        if !self.runs.iter().any(|r| r.key() == run && r.is_asking()) {
            return Err(ActionError::NothingToDo);
        }
        let label = self.label_of(&run);
        if !self.answer_agent(&run, &text) {
            return Err(ActionError::InvalidInput);
        }
        Ok(vec![Note::coded(
            Target::Run(run),
            msg!(
                CoreActionLogAgentAnswered,
                label = label,
                text = text.trim()
            ),
        )])
    }

    pub(super) fn perform_steer_agent(
        &mut self,
        run: String,
        text: String,
    ) -> Result<Vec<Note>, ActionError> {
        if !self
            .runs
            .iter()
            .any(|r| r.key() == run && r.is_active() && !r.is_awaiting())
        {
            return Err(ActionError::NothingToDo);
        }
        let label = self.label_of(&run);
        if !self.steer_agent(&run, &text) {
            return Err(ActionError::InvalidInput);
        }
        Ok(vec![Note::coded(
            Target::Run(run),
            msg!(CoreActionLogAgentSteered, label = label, text = text.trim()),
        )])
    }

    pub(super) fn perform_pause_agent(&mut self, run: String) -> Result<Vec<Note>, ActionError> {
        let label = self.label_of(&run);
        if !self.pause_agent(&run) {
            return Err(ActionError::NothingToDo);
        }
        Ok(vec![Note::coded(
            Target::Run(run),
            msg!(CoreActionLogAgentPaused, label = label),
        )])
    }

    pub(super) fn perform_resume_agent(&mut self, run: String) -> Result<Vec<Note>, ActionError> {
        let label = self.label_of(&run);
        if self.runs.iter().any(|r| r.key() == run && r.is_paused())
            && let Some(max) = self.parallel_full()
        {
            return Err(ActionError::ParallelLimit(max));
        }
        if !self.resume_agent(&run) {
            return Err(ActionError::NothingToDo);
        }
        Ok(vec![Note::coded(
            Target::Run(run),
            msg!(CoreActionLogAgentResumed, label = label),
        )])
    }

    pub(super) fn perform_report_agent(
        &mut self,
        run: String,
        report: AgentReport,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        let effect = self
            .report_agent(&run, report, at_ms)
            .ok_or(ActionError::NothingToDo)?;
        let label = self.label_of(&run);
        let mut notes: Vec<Note> = effect
            .notes
            .into_iter()
            .map(|msg| Note::coded(Target::Run(run.clone()), msg).by(Actor::Agent))
            .collect();
        if effect.finished {
            notes.push(
                Note::coded(
                    Target::Run(run.clone()),
                    msg!(CoreActionLogRunFinished, label = label),
                )
                .by(Actor::Agent),
            );
        }
        // Messages without a sentence (file list, hints delivered) are not a log entry.
        Ok(notes)
    }

    pub(super) fn perform_close_agent_run(
        &mut self,
        run: String,
        closure: Closure,
    ) -> Result<Vec<Note>, ActionError> {
        let label = self
            .close_run(&run, closure)
            .ok_or(ActionError::NothingToDo)?;
        Ok(vec![Note::coded(
            Target::Run(run),
            msg!(
                CoreActionLogRunClosed,
                label = label,
                what = labels::closure(closure)
            ),
        )])
    }

    pub(super) fn perform_remove_agent_run(
        &mut self,
        run: String,
    ) -> Result<Vec<Note>, ActionError> {
        let label = self.remove_run(&run).map_err(|active| {
            if active {
                ActionError::RunStillActive(self.label_of(&run))
            } else {
                ActionError::NothingToDo
            }
        })?;
        Ok(vec![Note::coded(
            Target::Run(run),
            msg!(CoreActionLogRunRemoved, label = label),
        )])
    }

    pub(super) fn perform_remove_finished_runs(&mut self) -> Result<Vec<Note>, ActionError> {
        let keys = self.remove_finished_runs();
        let Some(first) = keys.first().cloned() else {
            return Err(ActionError::NothingToDo);
        };
        Ok(vec![Note::coded(
            Target::Run(first),
            msg!(CoreActionLogRunsRemoved, n = keys.len()),
        )])
    }

    pub(super) fn perform_address_review_threads(
        &mut self,
        review: PrKey,
        options: AgentOptions,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        self.address_review_threads(&review, options)
            .map_err(agent_error)?;
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(CoreActionLogThreadsRequested, review = review.to_string()),
        )])
    }

    pub(super) fn perform_start_pr_review(
        &mut self,
        review: PrKey,
        language: Option<cockpit_i18n::Lang>,
        repo_language: Option<cockpit_i18n::Lang>,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        self.review_pr_with_agent(&review, language, repo_language)
            .map_err(agent_error)?;
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(CoreActionLogPrReviewRequested, review = review.to_string()),
        )])
    }

    pub(super) fn perform_fix_failed_checks(
        &mut self,
        review: PrKey,
        options: AgentOptions,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        self.fix_failed_checks(&review, options)
            .map_err(agent_error)?;
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(CoreActionLogChecksFixRequested, review = review.to_string()),
        )])
    }

    pub(super) fn perform_create_pull_request(
        &mut self,
        run: String,
        title: String,
        body: String,
        pull: Option<PullRequestRef>,
        repo: String,
    ) -> Result<Vec<Note>, ActionError> {
        self.check_pull_request(&run, &repo)?;
        if title.trim().is_empty() || title.chars().count() > 256 || body.chars().count() > 60_000 {
            return Err(ActionError::InvalidInput);
        }
        // Without the answer of GitHub nothing is booked (the backend fills it in).
        let pull = pull.ok_or(ActionError::InvalidInput)?;
        let label = self.label_of(&run);
        let n = pull.number;
        let text = if pull.existed {
            msg!(CoreActionLogPrExisted, n = n, label = label)
        } else {
            msg!(CoreActionLogPrCreated, n = n, label = label)
        };
        self.pull_request_created(&run, &repo, pull);
        Ok(vec![Note::coded(Target::Run(run), text)])
    }

    pub(super) fn perform_resume_stopped(
        &mut self,
        run: String,
        extra: u64,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        let reason = self
            .resume_stopped(&run, extra, at_ms)
            .map_err(|e| match e {
                ResumeError::NotStopped => ActionError::NothingToDo,
                ResumeError::ParallelLimit(n) => ActionError::ParallelLimit(n),
                ResumeError::StillReached(r) => ActionError::LimitStillReached(r),
            })?;
        let text = if extra == 0 {
            msg!(CoreActionLogResumed, run = &run)
        } else {
            msg!(
                CoreActionLogResumedRaised,
                run = &run,
                what = labels::raises(reason),
                amount = labels::raise_amount(reason, extra)
            )
        };
        Ok(vec![Note::coded(Target::Run(run.clone()), text)])
    }

    /// The name of the run for log sentences: the ticket key, for follow-up jobs with
    /// plain text. An unknown key stays as it is.
    pub(super) fn label_of(&self, key: &str) -> Msg {
        self.runs
            .iter()
            .find(|r| r.key() == key)
            .map_or_else(|| Msg::raw(key), labels::run_label)
    }
}
