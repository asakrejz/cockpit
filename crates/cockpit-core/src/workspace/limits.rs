use super::*;

impl Workspace {
    // ---------- Consumption and limits ----------

    /// Consumption on the UTC day of `now_ms`, across all runs.
    pub fn spent_today(&self, now_ms: u64) -> Usage {
        let from = day_start(now_ms);
        sum_between(&self.spend, from, from + 86_400_000)
    }

    /// Consumption in the UTC month of `now_ms`, across all runs.
    pub fn spent_month(&self, now_ms: u64) -> Usage {
        sum_between(&self.spend, month_start(now_ms), next_month_start(now_ms))
    }

    /// Runs that are currently working or waiting for an answer in the middle of a step.
    pub fn consuming_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_consuming()).count()
    }

    /// Is the limit of parallel runs reached? Then the number, otherwise `None`.
    pub fn parallel_full(&self) -> Option<u32> {
        let max = self.limits.max_parallel;
        (self.consuming_agents() >= max as usize).then_some(max)
    }

    /// Sets prices and limits. `false` if they are invalid or nothing changes.
    pub fn set_limits(&mut self, limits: AgentLimits) -> bool {
        if !limits.is_valid() || limits == self.limits {
            return false;
        }
        self.limits = limits;
        true
    }

    pub(super) fn charge(&mut self, at_ms: u64, ticket: &str, usage: Usage) {
        self.spend.push(Charge {
            at_ms,
            ticket: ticket.to_string(),
            usage,
        });
        let oldest = at_ms.saturating_sub(KEEP_DAYS * 86_400_000);
        self.spend.retain(|c| c.at_ms >= oldest);
        if self.spend.len() > MAX_CHARGES {
            let excess = self.spend.len() - MAX_CHARGES;
            self.spend.drain(..excess);
        }
    }

    /// Resumes a stopped run after the limit was raised by `extra` (cents for
    /// budgets, minutes for the time limit). The monthly budget applies to all runs and rises in the
    /// settings; budget and time limit of the run rise only for this run. If the limit is
    /// still reached afterwards or too many runs are already working, everything stays as it was.
    /// Returns which limit was raised.
    pub fn resume_stopped(
        &mut self,
        ticket_key: &str,
        extra: u64,
        now_ms: u64,
    ) -> Result<StopReason, ResumeError> {
        let idx = self
            .runs
            .iter()
            .position(|r| r.key() == ticket_key && r.is_stopped())
            .ok_or(ResumeError::NotStopped)?;
        if let Some(max) = self.parallel_full() {
            return Err(ResumeError::ParallelLimit(max));
        }
        let reason = self.runs[idx].stop_reason().unwrap_or(StopReason::Time);
        let mut limits = self.limits.clone();
        let mut run = self.runs[idx].clone();
        if reason == StopReason::MonthBudget {
            limits.month_budget_cents = limits.month_budget_cents.map(|b| b.saturating_add(extra));
        }
        run.resume_after_stop(extra);
        if let Some(still) = limits.reached(&run, self.spent_month(now_ms).cost_cents) {
            return Err(ResumeError::StillReached(still));
        }
        self.limits = limits;
        self.runs[idx] = run;
        self.set_agent_note(ticket_key, cockpit_i18n::msg!(CoreWorkspaceAgentWorking));
        Ok(reason)
    }

    // ---------- Rules per repo ----------

    /// The repos known from the data, for the choice of rules: those with rules, and every repo
    /// that reviews, entries and runs name. (Rules can be set for any repo
    /// of the form `owner/name`; the interface adds the configured ones.)
    pub fn repos(&self) -> Vec<String> {
        let mut repos: Vec<String> = Vec::new();
        let known = self
            .repo_rules
            .keys()
            .cloned()
            .chain(self.reviews.iter().map(|r| r.repo.clone()))
            .chain(self.items.iter().filter_map(|i| i.repo.clone()))
            .chain(self.runs.iter().map(|r| r.repo.clone()));
        for repo in known {
            if !repo.is_empty() && !repos.contains(&repo) {
                repos.push(repo);
            }
        }
        repos
    }

    /// The rules of a repo. Without an entry there are none.
    pub fn rules_for(&self, repo: &str) -> RepoRules {
        self.repo_rules.get(repo).cloned().unwrap_or_default()
    }

    /// Sets the rules of a repo (cleaned up). `false` if the repo is unknown or
    /// nothing changes. Waiting plans get the new rules immediately, running runs
    /// keep those they started with.
    pub fn set_repo_rules(&mut self, repo: &str, rules: RepoRules) -> bool {
        if !repo_name_valid(repo) {
            return false;
        }
        let rules = rules.cleaned();
        if self.rules_for(repo) == rules {
            return false;
        }
        for run in self
            .runs
            .iter_mut()
            .filter(|r| r.is_awaiting() && r.repo == repo)
        {
            run.rules = rules.clone();
        }
        self.repo_rules.insert(repo.to_string(), rules);
        true
    }

    /// Paused agents.
    pub fn paused_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_paused()).count()
    }
}
