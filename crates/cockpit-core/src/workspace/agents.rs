use super::*;

impl Workspace {
    // ---------- Agents ----------

    /// The run for a ticket. If there is none yet, a ready-to-start one is returned.
    pub fn run(&self, ticket_key: &str) -> Option<AgentRun> {
        if let Some(run) = self.runs.iter().find(|r| r.key() == ticket_key) {
            let mut run = run.clone();
            // A ready-to-start run always shows the current rules.
            if run.is_idle() {
                self.apply_rules(&mut run);
            }
            return Some(run);
        }
        let item = self.item_by_key(ticket_key).filter(|i| i.is_ticket())?;
        let mut run = AgentRun::new(&item.key, &item.title);
        self.apply_rules(&mut run);
        run.comments = item.comments.len();
        run.files = demo::agent_changes(&item.key)
            .into_iter()
            .map(|f| f.name)
            .collect();
        Some(run)
    }

    pub(super) fn apply_rules(&self, run: &mut AgentRun) {
        run.repo = self.repo_of(&run.ticket_key).to_string();
        run.rules = self.rules_for(&run.repo);
    }

    /// Commissions the agent with a ticket.
    pub fn start_agent(
        &mut self,
        ticket_key: &str,
        options: AgentOptions,
    ) -> Result<(), AgentError> {
        let mut run = self.run(ticket_key).ok_or(AgentError::UnknownTicket)?;
        // The repo the user (or the backend, by the routing rules) chose wins over the default.
        if let Some(repo) = options.repo.clone().filter(|r| !r.is_empty()) {
            run.rules = self.rules_for(&repo);
            run.repo = repo;
        }
        // Follow-up jobs start via the review, not via a ticket.
        if run.is_followup() {
            return Err(AgentError::UnknownTicket);
        }
        if run.is_active() {
            return Err(AgentError::AlreadyRunning);
        }
        run.start(options, self.next_pr_number());
        // A run that works immediately needs a free slot.
        if run.is_consuming()
            && let Some(max) = self.parallel_full()
        {
            return Err(AgentError::ParallelLimit(max));
        }
        let waiting = run.is_awaiting();
        match self.runs.iter_mut().find(|r| r.key() == ticket_key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket_key) {
            // If the agent is still waiting for approval, the ticket stays unchanged.
            if waiting {
                item.agent_note = Some("Der Agent wartet auf die Freigabe seines Plans".into());
            } else {
                item.status = TicketStatus::InProgress;
                item.agent_note = Some("Der Agent arbeitet an der Umsetzung".into());
            }
        }
        Ok(())
    }

    pub(super) fn active_run_mut(&mut self, key: &str) -> Option<&mut AgentRun> {
        self.runs
            .iter_mut()
            .find(|r| r.key() == key && r.is_active())
    }

    /// The entry the run hangs on: for follow-up jobs the one with the pull request.
    pub(super) fn run_item_mut(&mut self, key: &str) -> Option<&mut Item> {
        let item_key = self
            .runs
            .iter()
            .find(|r| r.key() == key)
            .map(|r| r.ticket_key.clone())?;
        self.items.iter_mut().find(|i| i.matches_run_key(&item_key))
    }

    pub(super) fn clear_agent_note(&mut self, key: &str) {
        if let Some(item) = self.run_item_mut(key) {
            item.agent_note = None;
        }
    }

    /// A follow-up job that no longer runs leaves no run behind. (For the ticket
    /// the ready run stays, as before.)
    pub(super) fn drop_idle_followups(&mut self) {
        // A failed review run stays, so that the review page can say why it ended.
        self.runs.retain(|r| {
            !(r.is_followup()
                && r.is_idle()
                && !(matches!(r.kind, RunKind::ReviewPr { .. }) && r.ended.is_some()))
        });
    }

    /// Cancels a waiting or running agent.
    /// The pull request of the run is merged or closed: the run becomes „erledigt“. `None` if
    /// the run does not wait for this (unknown, working, or already done with).
    pub fn close_run(&mut self, key: &str, closure: Closure) -> Option<String> {
        let run = self.runs.iter_mut().find(|r| r.key() == key)?;
        let label = run.label();
        if !run.close(closure) {
            return None;
        }
        self.clear_agent_note(key);
        Some(label)
    }

    /// The runs whose pull request the backend should ask GitHub about (see
    /// [`AgentRun::pr_to_watch`]), with the key of the run.
    pub fn runs_to_watch(&self) -> Vec<(String, PrKey)> {
        self.runs
            .iter()
            .filter_map(|r| r.pr_to_watch().map(|k| (r.key(), k)))
            .collect()
    }

    pub fn cancel_agent(&mut self, ticket_key: &str) {
        let Some(run) = self.active_run_mut(ticket_key) else {
            return;
        };
        run.reset();
        self.clear_agent_note(ticket_key);
        self.drop_idle_followups();
    }

    /// Approves the plan: the agent gets going, the ticket goes to „In Arbeit“.
    pub fn approve_plan(&mut self, ticket_key: &str) -> bool {
        let followup = self
            .runs
            .iter()
            .any(|r| r.key() == ticket_key && r.is_followup());
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::approve)
        {
            return false;
        }
        if let Some(item) = self.run_item_mut(ticket_key) {
            // A follow-up job does not change the ticket's status.
            if !followup {
                item.status = TicketStatus::InProgress;
            }
            item.agent_note = Some("Der Agent arbeitet an der Umsetzung".into());
        }
        true
    }

    /// Rejects the plan. The run ends without traces.
    pub fn reject_plan(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::reject)
        {
            return false;
        }
        self.clear_agent_note(ticket_key);
        self.drop_idle_followups();
        true
    }

    /// Requests a new plan with a note.
    pub fn revise_plan(&mut self, ticket_key: &str, note: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.revise(note))
    }

    pub(super) fn set_agent_note(&mut self, key: &str, note: &str) {
        if let Some(item) = self.run_item_mut(key) {
            item.agent_note = Some(note.into());
        }
    }

    /// Answers the agent's question; it keeps working afterwards.
    pub fn answer_agent(&mut self, ticket_key: &str, text: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(|r| r.answer(text))
        {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        true
    }

    /// Sends the running agent a hint for the next step.
    pub fn steer_agent(&mut self, ticket_key: &str, text: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.steer(text))
    }

    /// Stops a running agent.
    pub fn pause_agent(&mut self, ticket_key: &str) -> bool {
        if !self.active_run_mut(ticket_key).is_some_and(AgentRun::pause) {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent ist pausiert");
        true
    }

    /// Resumes a paused agent.
    pub fn resume_agent(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::resume)
        {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        true
    }

    /// Lets every running agent complete a step or ask a question.
    /// Returns the keys of the runs that finished in the process. Without a clock: bookings
    /// carry the time 0. Whoever knows the time takes [`Workspace::advance_agents_at`].
    pub fn advance_agents(&mut self) -> Vec<String> {
        self.advance_agents_at(0)
    }

    /// Like [`Workspace::advance_agents`], with the time of the tick (milliseconds since 1970).
    /// The consumption of each step goes into the ledger; if a limit is reached, the run stops.
    pub fn advance_agents_at(&mut self, now_ms: u64) -> Vec<String> {
        let mut finished = Vec::new();
        let mut asking = Vec::new();
        let mut diagnosed = Vec::new();
        let mut stopped = Vec::new();
        let mut charges = Vec::new();
        let limits = self.limits.clone();
        let mut month_spent = self.spent_month(now_ms).cost_cents;
        for run in &mut self.runs {
            let before = run.step_usage.len();
            let tick = run.tick_with(&limits, month_spent);
            // Newly reported consumption belongs in the ledger.
            if run.step_usage.len() > before
                && let Some(usage) = run.step_usage.last().copied()
            {
                month_spent = month_spent.saturating_add(usage.cost_cents);
                charges.push((run.ticket_key.clone(), usage));
            }
            match tick {
                Tick::Finished => finished.push(run.clone()),
                Tick::Asked => asking.push(run.key()),
                Tick::Diagnosed => diagnosed.push(run.key()),
                Tick::Stopped => stopped.push((run.key(), run.stop_reason())),
                Tick::Stepped | Tick::Idle => {}
            }
        }
        for (key, usage) in charges {
            self.charge(now_ms, &key, usage);
        }
        for (key, reason) in &stopped {
            if let Some(reason) = reason {
                self.set_agent_note(
                    key,
                    &format!("Der Agent ist angehalten: {}", reason.label()),
                );
            }
        }
        for key in &asking {
            self.set_agent_note(key, "Der Agent hat eine Frage an dich");
        }
        for key in &diagnosed {
            self.diagnose_run(key);
        }
        for run in &finished {
            if run.is_followup() {
                self.finish_followup(run);
            } else {
                self.publish(run);
            }
        }
        finished.into_iter().map(|r| r.key()).collect()
    }

    /// Books a report of the real agent (from the container) on run `key`: state,
    /// consumption in the ledger, note on the ticket. `None` if the report does not fit – then
    /// everything stays as it was.
    pub fn report_agent(
        &mut self,
        key: &str,
        report: AgentReport,
        now_ms: u64,
    ) -> Option<ReportEffect> {
        let limits = self.limits.clone();
        let month_spent = self.spent_month(now_ms).cost_cents;
        let run = self
            .runs
            .iter_mut()
            .find(|r| r.key() == key && r.is_active())?;
        let ticket = run.ticket_key.clone();
        if matches!(
            report,
            AgentReport::ReviewComment { .. } | AgentReport::ReviewSummary { .. }
        ) && !self.book_review_report(key, &report)
        {
            return None;
        }
        let run = self
            .runs
            .iter_mut()
            .find(|r| r.key() == key && r.is_active())?;
        let effect = run.report(report, &limits, month_spent)?;
        let run = run.clone();
        if let Some(usage) = effect.usage {
            self.charge(now_ms, &ticket, usage);
        }
        if effect.failed {
            self.clear_agent_note(key);
            self.drop_idle_followups();
        } else if effect.finished {
            self.finish_real(&run);
        } else if let Some(reason) = effect.stopped {
            self.set_agent_note(
                key,
                &format!("Der Agent ist angehalten: {}", reason.label()),
            );
        } else if effect.asking {
            self.set_agent_note(key, "Der Agent hat eine Frage an dich");
        } else if run.is_awaiting() {
            self.set_agent_note(
                key,
                if run.is_drafting() {
                    "Der Agent erarbeitet seinen Plan"
                } else {
                    "Der Agent wartet auf die Freigabe seines Plans"
                },
            );
        } else if run.is_running() {
            self.set_agent_note(key, "Der Agent arbeitet an der Umsetzung");
        }
        Some(effect)
    }

    /// The real agent is done: the new commits are on the branch on GitHub. The pull request is only
    /// created after your approval (`Action::CreatePullRequest`); a follow-up job continues on
    /// the branch of an existing pull request.
    pub(super) fn finish_real(&mut self, run: &AgentRun) {
        let branch = run.branch();
        if matches!(run.kind, RunKind::ReviewPr { .. }) {
            self.finish_review_run(run);
            return;
        }
        if let Some(number) = run.kind.review() {
            let what = match &run.kind {
                RunKind::Followup {
                    task: FollowupTask::FailedChecks { .. },
                    ..
                } => "Der Agent hat die roten Checks bearbeitet",
                _ => "Der Agent hat die Review-Kommentare bearbeitet",
            };
            if let Some(item) = self.pr_item_mut(number) {
                item.agent_note = Some(format!(
                    "{what} – neue Commits sind auf {branch} gepusht, bitte prüfen"
                ));
                item.reason = ReasonKind::AgentFinished;
                item.reason_text = format!("Agent fertig · neue Commits auf {branch}");
                item.time = "gerade eben".into();
                item.read = false;
            }
            return;
        }
        // The pull request number of a real run is only known once GitHub has created it.
        if let Some(slot) = self.runs.iter_mut().find(|r| r.key() == run.key()) {
            slot.pr_number = None;
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(branch.clone());
            item.agent_note = Some(format!(
                "Vom Agenten umgesetzt – Branch {branch} ist gepusht, den Pull Request legst du mit einem Klick an"
            ));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Branch {branch} gepusht");
            item.time = "gerade eben".into();
            item.read = false;
        }
    }

    // ---------- Pull request for the branch of a finished real run ----------

    /// Title and text the pull request would get, for the preview. `None` if the run does not exist.
    pub fn pull_request_draft(&self, run_key: &str) -> Option<PullRequestDraft> {
        let run = self.runs.iter().find(|r| r.key() == run_key)?;
        let files: Vec<String> = run
            .files
            .iter()
            .take(20)
            .map(|f| format!("- `{f}`"))
            .collect();
        let mut body = format!("## Was\n{}\n", run.summary.trim());
        body.push_str(&format!(
            "\n## Ticket\n{}: {}\n",
            run.ticket_key, run.ticket_title
        ));
        if !files.is_empty() {
            body.push_str(&format!(
                "\n## Geänderte Dateien ({})\n{}\n",
                run.files.len(),
                files.join("\n")
            ));
        }
        body.push_str("\n---\nVom Cockpit-Agenten umgesetzt. Gemergt wird von Hand.\n");
        Some(PullRequestDraft {
            title: format!("{}: {}", run.ticket_key, run.ticket_title),
            body,
            head: run.branch(),
            base: run.options.base_branch.clone(),
        })
    }

    /// May a pull request be created for this run? The run must be a finished real run
    /// (the backend pushed its branch before reporting it done) without a pull request.
    pub fn check_pull_request(&self, run_key: &str) -> Result<(), ActionError> {
        let run = self
            .runs
            .iter()
            .find(|r| r.key() == run_key)
            .ok_or(ActionError::NothingToDo)?;
        if run.kind != RunKind::Implement || !run.real || !run.is_done() {
            return Err(ActionError::PullRequestNotReady);
        }
        if let Some(p) = &run.pull_request {
            return Err(ActionError::PullRequestExists(p.number));
        }
        Ok(())
    }

    /// GitHub has created the pull request (or the existing one was found): book it on the run.
    pub(crate) fn pull_request_created(&mut self, run_key: &str, pull: PullRequestRef) {
        let Some(run) = self.runs.iter_mut().find(|r| r.key() == run_key) else {
            return;
        };
        run.pr_number = Some(pull.number);
        let ticket = run.ticket_key.clone();
        let number = pull.number;
        run.pull_request = Some(pull);
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket) {
            item.agent_note = Some(format!(
                "Pull Request #{number} ist angelegt – bitte prüfen"
            ));
        }
    }
}

/// Title and text of a pull request that is about to be created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestDraft {
    pub title: String,
    pub body: String,
    /// The branch of the agent.
    pub head: String,
    /// The branch the pull request goes against.
    pub base: String,
}
