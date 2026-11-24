use crate::agent::{RunRepo, RunRepoState};
use cockpit_i18n::msg;

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
        Some(run)
    }

    pub(super) fn apply_rules(&self, run: &mut AgentRun) {
        run.rules = self.rules_for(&run.repo);
    }

    /// Commissions the agent with a ticket.
    pub fn start_agent(
        &mut self,
        ticket_key: &str,
        options: AgentOptions,
    ) -> Result<(), AgentError> {
        let mut run = self.run(ticket_key).ok_or(AgentError::UnknownTicket)?;
        // The repo the user (or the backend, from the default) chose.
        let chosen = options.chosen_repos();
        if let Some(first) = chosen.first() {
            run.rules = self.rules_for(first);
            run.repo.clone_from(first);
            run.repos = chosen
                .iter()
                .enumerate()
                .map(|(i, r)| RunRepo::from_options(&options, r, self.rules_for(r), i == 0))
                .collect();
        }
        // Follow-up jobs start via the review, not via a ticket.
        if run.is_followup() {
            return Err(AgentError::UnknownTicket);
        }
        if run.is_active() {
            return Err(AgentError::AlreadyRunning);
        }
        run.start(options);
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
                item.set_note(msg!(CoreWorkspaceAgentWaitingPlan));
            } else {
                item.status = TicketStatus::InProgress;
                item.set_note(msg!(CoreWorkspaceAgentWorking));
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
            item.clear_note();
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
    pub fn close_run(&mut self, key: &str, closure: Closure) -> Option<cockpit_i18n::Msg> {
        let run = self.runs.iter_mut().find(|r| r.key() == key)?;
        let label = crate::action::labels::run_label(run);
        if !run.close(closure) {
            return None;
        }
        self.clear_agent_note(key);
        Some(label)
    }

    /// Removes the run with this key from the list. The consumption (`spend`) is a separate
    /// ledger and stays. `Err(true)` if the run still works or waits, `Err(false)` if there is
    /// no such run. Returns the label of the removed run.
    pub fn remove_run(&mut self, key: &str) -> Result<cockpit_i18n::Msg, bool> {
        let run = self.runs.iter().find(|r| r.key() == key).ok_or(false)?;
        if !run.is_removable() {
            return Err(true);
        }
        let label = crate::action::labels::run_label(run);
        self.clear_agent_note(key);
        self.runs.retain(|r| r.key() != key);
        Ok(label)
    }

    /// Removes every run in a final state; returns the keys that went.
    pub fn remove_finished_runs(&mut self) -> Vec<String> {
        let keys: Vec<String> = self
            .runs
            .iter()
            .filter(|r| r.is_removable())
            .map(|r| r.key())
            .collect();
        for key in &keys {
            self.clear_agent_note(key);
        }
        self.runs.retain(|r| !r.is_removable());
        keys
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
            item.set_note(msg!(CoreWorkspaceAgentWorking));
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

    pub(super) fn set_agent_note(&mut self, key: &str, note: cockpit_i18n::Msg) {
        if let Some(item) = self.run_item_mut(key) {
            item.set_note(note);
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
        self.set_agent_note(ticket_key, msg!(CoreWorkspaceAgentWorking));
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
        self.set_agent_note(ticket_key, msg!(CoreWorkspaceAgentPaused));
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
        self.set_agent_note(ticket_key, msg!(CoreWorkspaceAgentWorking));
        true
    }

    /// Books a report of the agent (from the container) on run `key`: state,
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
                msg!(CoreWorkspaceAgentStopped, reason = reason.label_msg()),
            );
        } else if effect.asking {
            self.set_agent_note(key, msg!(CoreWorkspaceAgentQuestion));
        } else if run.is_awaiting() {
            self.set_agent_note(
                key,
                if run.is_drafting() {
                    msg!(CoreWorkspaceAgentDrafting)
                } else {
                    msg!(CoreWorkspaceAgentWaitingPlan)
                },
            );
        } else if run.is_running() {
            self.set_agent_note(key, msg!(CoreWorkspaceAgentWorking));
        }
        Some(effect)
    }

    /// The agent is done: the new commits are on the branch on GitHub. The pull request is only
    /// created after your approval (`Action::CreatePullRequest`); a follow-up job continues on
    /// the branch of an existing pull request.
    pub(super) fn finish_real(&mut self, run: &AgentRun) {
        let branch = run.branch();
        if matches!(run.kind, RunKind::ReviewPr { .. }) {
            self.finish_review_run(run);
            return;
        }
        if let Some(number) = run.kind.review() {
            let note = match &run.kind {
                RunKind::Followup {
                    task: FollowupTask::FailedChecks { .. },
                    ..
                } => msg!(CoreWorkspaceAgentPushedChecks, branch = &branch),
                _ => msg!(CoreWorkspaceAgentPushedComments, branch = &branch),
            };
            if let Some(item) = self.pr_item_mut(number) {
                item.set_note(note);
                item.reason = ReasonKind::AgentFinished;
                item.reason_text = format!("Agent fertig · neue Commits auf {branch}");
                item.time = "gerade eben".into();
                item.read = false;
            }
            return;
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(branch.clone());
            item.set_note(msg!(CoreWorkspaceAgentBranchPushed, branch = &branch));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Branch {branch} gepusht");
            item.time = "gerade eben".into();
            item.read = false;
        }
    }

    // ---------- Pull request for the branch of a finished real run ----------

    /// The language of the text (pull request, commit message) the agent writes for `run`: the
    /// language detected from the ticket, else `repo_lang` (the repo's `pr_language`), else English.
    pub fn pr_text_language(
        &self,
        run: &AgentRun,
        repo_lang: cockpit_i18n::Lang,
    ) -> crate::language::LangChoice {
        let description = self
            .item_by_key(&run.ticket_key)
            .map_or("", |i| i.description.as_str());
        crate::language::choose_text(&run.ticket_title, description, Some(repo_lang))
    }

    /// Title and text the pull request in `repo` would get, for the preview. `None` if the run
    /// does not exist or does not work in that repo. An empty `repo` means the primary one.
    pub fn pull_request_draft(&self, run_key: &str, repo: &str) -> Option<PullRequestDraft> {
        let run = self.runs.iter().find(|r| r.key() == run_key)?;
        let repo = if repo.is_empty() {
            run.repo.as_str()
        } else {
            repo
        };
        let entry = run.repo_entry(repo);
        if entry.is_none() && !(repo == run.repo || run.repo.is_empty()) {
            return None;
        }
        // Only the files of this repo (a multi run keeps them apart; a single run has them in `files`).
        let own: Vec<String> = match &entry {
            Some(e) if !e.files.is_empty() => e.files.clone(),
            _ => run.files.clone(),
        };
        let files: Vec<String> = own.iter().take(20).map(|f| format!("- `{f}`")).collect();
        // The text follows the language of the ticket, else the repo (the team language), never the interface.
        let repo_lang = entry
            .as_ref()
            .filter(|_| run.is_multi_repo())
            .map_or(run.options.pr_language, |e| e.pr_language);
        let lang = self.pr_text_language(run, repo_lang).lang;
        let text = |msg: cockpit_i18n::Msg| msg.render(lang);
        let mut body = format!(
            "## {}\n{}\n",
            text(cockpit_i18n::msg!(CorePrWhat)),
            run.summary.trim()
        );
        body.push_str(&format!(
            "\n## {}\n{}: {}\n",
            text(cockpit_i18n::msg!(CorePrTicket)),
            run.ticket_key,
            run.ticket_title
        ));
        // A GitHub issue is closed by the merge: `Fixes` is GitHub's own keyword, so it stays English
        // whatever language the text has.
        if crate::project::parse_issue_key(&run.ticket_key).is_some() {
            body.push_str(&format!("\nFixes {}\n", run.ticket_key));
        }
        if !files.is_empty() {
            body.push_str(&format!(
                "\n## {}\n{}\n",
                text(cockpit_i18n::msg!(CorePrFiles, n = own.len())),
                files.join("\n")
            ));
        }
        // The sibling pull requests of the same run: linked if they exist, otherwise the branch.
        let siblings: Vec<String> = run
            .repo_list()
            .iter()
            .filter(|r| r.repo != repo && r.is_pushed())
            .map(|r| match &r.pull_request {
                Some(p) if !p.url.is_empty() => format!("- {}#{}: {}", r.repo, p.number, p.url),
                Some(p) => format!("- {}#{}", r.repo, p.number),
                None => format!("- {}: `{}`", r.repo, run.branch_in(&r.repo)),
            })
            .collect();
        if run.is_multi_repo() && !siblings.is_empty() {
            body.push_str(&format!(
                "\n## {}\n{}\n",
                text(cockpit_i18n::msg!(CorePrSiblings)),
                siblings.join("\n")
            ));
        }
        body.push_str(&format!(
            "\n---\n{}\n",
            text(cockpit_i18n::msg!(CorePrFooter))
        ));
        let base = entry
            .as_ref()
            .map(|e| e.base_branch.clone())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| run.options.base_branch.clone());
        Some(PullRequestDraft {
            title: format!("{}: {}", run.ticket_key, run.ticket_title),
            body,
            head: run.branch_in(repo),
            base,
        })
    }

    /// May a pull request be created for this run? The run must be a finished implementation
    /// (the backend pushed its branch before reporting it done) without a pull request.
    /// With several repos the pull request is created per repo: only a repo whose branch was
    /// pushed qualifies (not an unchanged or refused one), and each gets only one.
    pub fn check_pull_request(&self, run_key: &str, repo: &str) -> Result<(), ActionError> {
        let run = self
            .runs
            .iter()
            .find(|r| r.key() == run_key)
            .ok_or(ActionError::NothingToDo)?;
        if run.kind != RunKind::Implement || !run.is_done() {
            return Err(ActionError::PullRequestNotReady);
        }
        let repo = if repo.is_empty() {
            run.repo.as_str()
        } else {
            repo
        };
        if run.repos.is_empty() {
            if let Some(p) = &run.pull_request {
                return Err(ActionError::PullRequestExists(p.number));
            }
            return Ok(());
        }
        let entry = run
            .repos
            .iter()
            .find(|r| r.repo == repo)
            .ok_or(ActionError::PullRequestNotReady)?;
        if let Some(p) = &entry.pull_request {
            return Err(ActionError::PullRequestExists(p.number));
        }
        if entry.state != RunRepoState::Pushed {
            return Err(ActionError::PullRequestNotReady);
        }
        Ok(())
    }

    /// GitHub has created the pull request (or the existing one was found): book it on the run.
    pub(crate) fn pull_request_created(&mut self, run_key: &str, repo: &str, pull: PullRequestRef) {
        let Some(run) = self.runs.iter_mut().find(|r| r.key() == run_key) else {
            return;
        };
        let ticket = run.ticket_key.clone();
        let number = pull.number;
        let repo = if repo.is_empty() {
            run.repo.clone()
        } else {
            repo.to_string()
        };
        if run.repos.is_empty() {
            run.pr_number = Some(pull.number);
            run.pull_request = Some(pull);
        } else {
            run.book_pull_request(&repo, pull);
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket) {
            item.set_note(msg!(CoreWorkspacePrCreated, number = number));
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
