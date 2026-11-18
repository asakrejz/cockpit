use cockpit_i18n::{Msg, msg};

use super::*;

impl Workspace {
    pub(super) fn perform_agent(
        &mut self,
        action: Action,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::StartAgent { ticket, options } => {
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
            Action::CancelAgent { ticket } => {
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
            Action::ApprovePlan { ticket } => {
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
            Action::RejectPlan { ticket } => {
                let label = self.label_of(&ticket);
                if !self.reject_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::coded(
                    Target::Run(ticket),
                    msg!(CoreActionLogPlanRejected, label = label),
                )])
            }
            Action::RevisePlan { ticket, note } => {
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
            Action::AnswerAgent { run, text } => {
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
            Action::SteerAgent { run, text } => {
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
            Action::PauseAgent { run } => {
                let label = self.label_of(&run);
                if !self.pause_agent(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::coded(
                    Target::Run(run),
                    msg!(CoreActionLogAgentPaused, label = label),
                )])
            }
            Action::ResumeAgent { run } => {
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
            Action::AdvanceAgents => self.advance_logged(at_ms),
            Action::ReportAgent { run, report } => {
                let effect = self
                    .report_agent(&run, report, at_ms)
                    .ok_or(ActionError::NothingToDo)?;
                let label = self.label_of(&run);
                let mut notes: Vec<Note> = effect
                    .notes
                    .into_iter()
                    .map(|text| Note::new(Target::Run(run.clone()), text).by(Actor::Agent))
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
            Action::CloseAgentRun { run, closure } => {
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
            Action::AddressReview {
                review,
                comments,
                options,
            } => {
                self.need_open_review(&review)?;
                // Unknown comments report by name, done ones count like missing ones.
                if let Some(id) = comments.iter().find(|id| {
                    self.review(&review)
                        .is_some_and(|r| !r.line_comments.iter().any(|c| &c.id == *id))
                }) {
                    return Err(ActionError::UnknownComment(id.clone()));
                }
                let plan_first = options.plan_first;
                let n = comments.len();
                let key = self
                    .address_review(&review, &comments, options)
                    .map_err(ActionError::Agent)?;
                let mut notes = vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(
                        CoreActionLogAddressRequested,
                        comments = labels::comments(n),
                        review = review.to_string()
                    ),
                )];
                if plan_first {
                    notes.push(
                        Note::coded(
                            Target::Run(key),
                            msg!(CoreActionLogAddressPlan, review = review.to_string()),
                        )
                        .by(Actor::Agent),
                    );
                }
                Ok(notes)
            }
            Action::InvestigateCheck { review, check } => {
                self.need_open_review(&review)?;
                let known = self
                    .review(&review)
                    .is_some_and(|r| r.check_runs.iter().any(|c| c.name == check));
                if !known {
                    return Err(ActionError::UnknownCheck(check));
                }
                self.investigate_check(&review, &check)
                    .map_err(ActionError::Agent)?;
                Ok(vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(
                        CoreActionLogInvestigate,
                        check = check,
                        review = review.to_string()
                    ),
                )])
            }
            Action::ApplyFix { run } => {
                let label = self.label_of(&run);
                let pushes = self
                    .runs
                    .iter()
                    .find(|r| r.key() == run && r.is_diagnosed())
                    .and_then(|r| r.diagnosis.as_ref())
                    .is_none_or(|d| d.changes_code());
                if !self.accept_fix(&run) {
                    return Err(ActionError::NothingToDo);
                }
                let text = if pushes {
                    msg!(CoreActionLogFixApplied, label = label)
                } else {
                    msg!(CoreActionLogSuggestionApplied, label = label)
                };
                Ok(vec![Note::coded(Target::Run(run), text)])
            }
            Action::DiscardFix { run } => {
                let label = self.label_of(&run);
                if !self.discard_fix(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::coded(
                    Target::Run(run),
                    msg!(CoreActionLogFixDiscarded, label = label),
                )])
            }
            Action::AddressReviewThreads { review, options } => {
                self.need_open_review(&review)?;
                self.address_review_threads(&review, options)
                    .map_err(agent_error)?;
                Ok(vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(CoreActionLogThreadsRequested, review = review.to_string()),
                )])
            }
            Action::StartPrReview { review } => {
                self.need_open_review(&review)?;
                self.review_pr_with_agent(&review).map_err(agent_error)?;
                Ok(vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(CoreActionLogPrReviewRequested, review = review.to_string()),
                )])
            }
            Action::FixFailedChecks { review, options } => {
                self.need_open_review(&review)?;
                self.fix_failed_checks(&review, options)
                    .map_err(agent_error)?;
                Ok(vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(CoreActionLogChecksFixRequested, review = review.to_string()),
                )])
            }
            Action::CreatePullRequest {
                run,
                title,
                body,
                pull,
            } => {
                self.check_pull_request(&run)?;
                if title.trim().is_empty()
                    || title.chars().count() > 256
                    || body.chars().count() > 60_000
                {
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
                self.pull_request_created(&run, pull);
                Ok(vec![Note::coded(Target::Run(run), text)])
            }
            Action::ResumeStopped { run, extra } => {
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
            _ => unreachable!("agent: dispatched by perform"),
        }
    }

    /// The name of the run for log sentences: the ticket key, for follow-up jobs with
    /// plain text. An unknown key stays as it is.
    pub(super) fn label_of(&self, key: &str) -> Msg {
        self.runs
            .iter()
            .find(|r| r.key() == key)
            .map_or_else(|| Msg::raw(key), labels::run_label)
    }

    /// The tick of the agents, with one entry per completed step or asked question.
    fn advance_logged(&mut self, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        // Note beforehand which step is about to finish and which hints are waiting.
        let stepping: Vec<(String, usize, String, Option<PrKey>, bool, usize)> = self
            .runs
            .iter()
            .filter(|r| r.is_running() && !r.real)
            .map(|r| {
                let title = r.steps()[r.completed.min(STEPS - 1)].title.clone();
                (
                    r.key(),
                    r.completed,
                    title,
                    r.pr_key(),
                    r.options.draft_pr,
                    r.pending_hints.len(),
                )
            })
            .collect();
        if stepping.is_empty() {
            return Err(ActionError::NothingToDo);
        }
        self.advance_agents_at(at_ms);
        let mut notes = Vec::new();
        for (key, done, title, pr, draft, hints) in stepping {
            let Some(run) = self.runs.iter().find(|r| r.key() == key) else {
                continue;
            };
            let label = labels::run_label(run);
            let (asking, diagnosed) = (run.is_asking(), run.is_diagnosed());
            let kind = run.kind.clone();
            if hints > 0 {
                notes.push(
                    Note::coded(
                        Target::Run(key.clone()),
                        msg!(
                            CoreActionLogHintsConsidered,
                            label = label.clone(),
                            hints = msg!(CoreActionHints, n = hints)
                        ),
                    )
                    .by(Actor::Agent),
                );
            }
            // A limit stopped the run before it did the step.
            if let Some(reason) = run.stop_reason() {
                notes.push(Note::coded(
                    Target::Run(key.clone()),
                    msg!(
                        CoreActionLogStopped,
                        label = label,
                        reason = labels::stop(reason)
                    ),
                ));
                continue;
            }
            let last = done + 1 == STEPS;
            notes.push(match (asking, diagnosed, last, pr, kind) {
                (true, ..) => Note::coded(
                    Target::Run(key.clone()),
                    msg!(CoreActionLogAsking, label = label),
                ),
                (_, true, ..) => Note::coded(
                    Target::Run(key.clone()),
                    msg!(CoreActionLogDiagnosed, label = label),
                ),
                (_, _, true, Some(n), RunKind::AddressReview { comments, .. }) => Note::coded(
                    Target::Review(n.clone()),
                    msg!(
                        CoreActionLogCommentsDone,
                        review = n.to_string(),
                        comments = labels::comments(comments.len())
                    ),
                ),
                (_, _, true, Some(n), RunKind::Diagnose { check, .. }) => Note::coded(
                    Target::Review(n.clone()),
                    msg!(
                        CoreActionLogCheckRestartedAfterFix,
                        review = n.to_string(),
                        check = check
                    ),
                ),
                (_, _, true, Some(n), RunKind::Implement) => Note::coded(
                    Target::Review(n.clone()),
                    if draft {
                        msg!(CoreActionLogPrMadeDraft, key = &key, review = n.to_string())
                    } else {
                        msg!(CoreActionLogPrMade, key = &key, review = n.to_string())
                    },
                ),
                _ => Note::coded(
                    Target::Run(key.clone()),
                    msg!(CoreActionLogStep, label = label, title = title),
                ),
            });
        }
        Ok(notes)
    }
}
