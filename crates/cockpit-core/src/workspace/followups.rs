use super::*;

impl Workspace {
    // ---------- Follow-up jobs: implement review comments, investigate red checks ----------

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
        run.start(options, 0);
        let key = run.key();
        let waiting = run.is_awaiting();
        let note = match &run.kind {
            _ if waiting => "Der Agent wartet auf die Freigabe seines Plans",
            RunKind::Diagnose { .. } => "Der Agent untersucht einen roten Check",
            RunKind::Followup {
                task: FollowupTask::FailedChecks { .. },
                ..
            } => "Der Agent behebt rote Checks",
            _ => "Der Agent setzt Review-Kommentare um",
        };
        let pr = run.pr_key();
        match self.runs.iter_mut().find(|r| r.key() == key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = pr.and_then(|k| self.pr_item_mut(k)) {
            item.agent_note = Some(note.into());
        }
        key
    }

    /// Commissions the agent to implement comments of a review. Only open
    /// line comments of this review count. Returns the key of the run (`review-91`).
    ///
    /// The run pushes to the pull request's branch; there is no new pull request.
    pub fn address_review(
        &mut self,
        review: impl Into<PrKey>,
        comments: &[String],
        options: AgentOptions,
    ) -> Result<String, AgentError> {
        let review = review.into();
        let r = self.check_followup(&review)?;
        let review = r.key();
        let mut files: Vec<String> = Vec::new();
        for id in comments {
            let c = r
                .line_comments
                .iter()
                .find(|c| c.id == *id && c.is_open())
                .ok_or(AgentError::NoComments)?;
            if !files.contains(&c.file) {
                files.push(c.file.clone());
            }
        }
        if comments.is_empty() {
            return Err(AgentError::NoComments);
        }
        let mut ids: Vec<String> = Vec::new();
        for id in comments {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let mut run = AgentRun::followup(
            RunKind::AddressReview {
                review: review.clone(),
                comments: ids,
            },
            &self.item_key_of_pr(&review),
            &title,
            &branch,
        );
        run.files = files;
        Ok(self.start_followup(run, options))
    }

    /// Commissions the agent to investigate a red check. It presents a diagnosis and
    /// changes nothing before you adopt the fix. Returns the key of the run.
    pub fn investigate_check(
        &mut self,
        review: impl Into<PrKey>,
        check: &str,
    ) -> Result<String, AgentError> {
        let review = review.into();
        let r = self.check_followup(&review)?;
        let review = r.key();
        let failed = r
            .check_runs
            .iter()
            .any(|c| c.name == check && c.status == Checks::Failed);
        if !failed {
            return Err(AgentError::CheckNotFailed);
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let run = AgentRun::followup(
            RunKind::Diagnose {
                review: review.clone(),
                check: check.to_string(),
            },
            &self.item_key_of_pr(&review),
            &title,
            &branch,
        );
        // An investigation changes nothing before you adopt the fix – it needs no plan.
        let options = AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        };
        Ok(self.start_followup(run, options))
    }

    /// Commissions the **real** agent to implement the review comments of the pull request. The
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

    /// Commissions the **real** agent to fix the red checks of the pull request. Returns the key
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

    /// The investigation has read the log: the diagnosis is created and attached to the run.
    pub(super) fn diagnose_run(&mut self, key: &str) {
        let Some((review, check)) =
            self.runs
                .iter()
                .find(|r| r.key() == key)
                .and_then(|r| match &r.kind {
                    RunKind::Diagnose { review, check } => Some((review.clone(), check.clone())),
                    _ => None,
                })
        else {
            return;
        };
        let diagnosis = self.review(&review).and_then(|r| {
            let c = r.check_runs.iter().find(|c| c.name == check)?;
            Some(diagnose(r, c))
        });
        let note = match &diagnosis {
            Some(d) => format!("Diagnose zu „{check}“ liegt vor: {}", d.cause),
            None => format!("Der Check „{check}“ ist nicht mehr da"),
        };
        if let Some(run) = self.runs.iter_mut().find(|r| r.key() == key) {
            run.diagnosis = diagnosis;
        }
        self.set_agent_note(key, &note);
    }

    /// Adopts the fix of the diagnosis: the agent applies it, pushes and restarts the check.
    pub fn accept_fix(&mut self, key: &str) -> bool {
        if !self.active_run_mut(key).is_some_and(AgentRun::accept_fix) {
            return false;
        }
        self.set_agent_note(key, "Der Agent wendet den Fix an");
        true
    }

    /// Discards the diagnosis: the run ends, the branch stays untouched.
    pub fn discard_fix(&mut self, key: &str) -> bool {
        if !self.runs.iter().any(|r| r.key() == key && r.is_diagnosed()) {
            return false;
        }
        if let Some(run) = self.active_run_mut(key) {
            run.reset();
        }
        self.clear_agent_note(key);
        self.drop_idle_followups();
        true
    }

    /// A follow-up job is done: the new commit is on the pull request's branch.
    pub(super) fn finish_followup(&mut self, run: &AgentRun) {
        let Some(number) = run.kind.review() else {
            return;
        };
        match &run.kind {
            RunKind::AddressReview { comments, .. } => {
                let key = run.key();
                let sha = short_sha(&format!("{key}:{}", comments.join(",")));
                let done = self.review_mut(&number).map_or(0, |r| {
                    // Change suggestions land in the diff, then the comments are done.
                    for id in comments {
                        r.apply_comment_suggestion(id);
                    }
                    let done = r.resolve_comments(comments, &key);
                    // The review is a new round: the old verdict no longer applies.
                    r.verdict = Verdict::Comment;
                    r.sent = false;
                    done
                });
                let what = match done {
                    1 => "1 Review-Kommentar".to_string(),
                    n => format!("{n} Review-Kommentare"),
                };
                if let Some(item) = self.pr_item_mut(&number) {
                    if let Some(pr) = item.pr.as_mut() {
                        pr.changes_requested = false;
                    }
                    item.agent_note = Some(format!("Neuer Commit {sha}: {what} umgesetzt"));
                    item.reason = ReasonKind::ReviewRequested;
                    item.reason_text = format!("Review angefragt · Agent hat {what} umgesetzt");
                    item.time = "gerade eben".into();
                    item.read = false;
                    item.comments.push(Comment::new(
                        Person::agent(),
                        "gerade eben",
                        &format!(
                            "Commit {sha} auf {}: {what} umgesetzt. Bitte noch einmal prüfen.",
                            run.branch()
                        ),
                    ));
                }
            }
            RunKind::Diagnose { check, .. } => {
                let fix = run.diagnosis.as_ref().and_then(|d| d.fix.clone());
                let pushed = self
                    .review_mut(&number)
                    .zip(fix.as_ref())
                    .is_some_and(|(r, fix)| {
                        let applied = r.apply_fix(fix);
                        if applied {
                            // The fix removes the error; a flaky check only needs the restart.
                            r.set_rerun(check, Checks::Passed);
                        }
                        applied
                    });
                self.restart_check(&number, check);
                let sha = short_sha(&format!("{}:{check}", run.key()));
                let note = if pushed {
                    format!("Fix {sha} gepusht, „{check}“ läuft erneut")
                } else {
                    format!("„{check}“ neu gestartet")
                };
                if let Some(item) = self.pr_item_mut(&number) {
                    item.agent_note = Some(note.clone());
                    if pushed {
                        item.comments.push(Comment::new(
                            Person::agent(),
                            "gerade eben",
                            &format!("Commit {sha} auf {}: {note}.", run.branch()),
                        ));
                    }
                }
            }
            RunKind::Implement | RunKind::Followup { .. } => {}
        }
    }

    /// A run is done: branch and pull request appear on the ticket, and the
    /// pull request waits as a review.
    pub(super) fn publish(&mut self, run: &AgentRun) {
        let Some(number) = run.pr_number else { return };
        let checks = if run.options.run_tests {
            Checks::Passed
        } else {
            Checks::Running
        };
        let tests = if run.options.run_tests {
            "Tests bestanden"
        } else {
            "Tests übersprungen"
        };

        let mut seen = None;
        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(run.branch());
            item.pr = Some(PrRef {
                number,
                repo: run.repo.clone(),
                draft: run.options.draft_pr,
                checks,
                changes_requested: false,
                merged: false,
                review_requested: true,
                author: String::new(),
                mine: false,
                requested_me: false,
                assigned_me: false,
            });
            // The pull request waits as a review – the same way as for any linked ticket.
            item.status = item
                .pr
                .as_ref()
                .map_or(TicketStatus::Review, PrRef::target_status);
            seen = Some((item.key.clone(), item.status));
            item.agent_note = Some(format!("Vom Agenten umgesetzt · {tests}"));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Pull Request #{number} erstellt");
            item.time = "gerade eben".into();
            item.read = false;
        }
        if let Some((key, status)) = seen {
            self.mark_status_seen(&key, status);
        }
        self.reviews.push(Review {
            number,
            repo: run.repo.clone(),
            title: run.ticket_title.clone(),
            description: format!(
                "Vom Agenten für {} umgesetzt · {tests}. Der Agent führt keinen Merge aus.",
                run.ticket_key
            ),
            author: Person::agent(),
            branch: run.branch(),
            base: run.options.base_branch.clone(),
            ticket_key: Some(run.ticket_key.clone()),
            checks,
            files: demo::agent_changes(&run.ticket_key),
            agent_reviewed: false,
            suggestions: Vec::new(),
            line_comments: Vec::new(),
            verdict: Verdict::Comment,
            sent: false,
            check_runs: {
                let mut runs = demo::passing_checks();
                // Without tests the test check is still running; the tick of the checks completes it.
                if checks == Checks::Running
                    && let Some(tests) = runs.last_mut()
                {
                    tests.status = Checks::Running;
                    tests.remaining = CHECK_STEPS;
                    tests.log = "$ cargo test\nwird ausgeführt …\n".into();
                }
                runs
            },
            merged: None,
        });
    }
}
