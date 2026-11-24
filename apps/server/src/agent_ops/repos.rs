//! Which repos the agent may work in and where a start request goes.

use super::super::agent_followup::github_message;
use super::*;
use cockpit_core::AgentOptions;
use cockpit_core::AgentRepo;
use cockpit_core::api::{AgentReposInput, ConfigChange};
use cockpit_core::branch::{BranchType, DEFAULT_PATTERN};
use cockpit_core::model::PrKey;
use cockpit_i18n::{Msg, msg};

impl AppState {
    /// The repos the agent may work in, with the default.
    pub(crate) fn agent_repos(&self) -> AgentRepos {
        let cfg = match self.inner.agent_repo_config.read() {
            Ok(c) => c.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let github: Vec<String> = self
            .sources()
            .github
            .as_ref()
            .map(|g| g.repos.clone())
            .unwrap_or_default();
        agent_run::repos_effective(&cfg.repos, cfg.default.as_deref(), &github)
    }

    /// May the agent work in this repo? Any `owner/name` the person has access to: the repo is
    /// chosen when the run starts (the clone fails if GitHub does not show it). A repo of the
    /// configuration needs no check either.
    pub(crate) fn repo_usable(&self, repo: &str) -> bool {
        agent_git::repo_name_ok(repo)
    }

    /// Does the configuration hold the settings of this repo (`[[agent.repos]]` or the default
    /// `[agent] repo`)? Blocking-free: only reads the kept configuration.
    pub(crate) fn repo_configured(&self, repo: &str) -> bool {
        let cfg = match self.inner.agent_repo_config.read() {
            Ok(c) => c.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        cfg.default.as_deref() == Some(repo) || cfg.repos.iter().any(|r| r.repo == repo)
    }

    /// The repo an action is about, if it names one: the choice of a start, or the repo of the
    /// pull request a follow-up or review works on.
    fn action_repos(&self, action: &Action) -> Vec<String> {
        let ws = self.inner.live.copy();
        let of_pr = |review: &PrKey| {
            ws.sync
                .pr(review)
                .map(|l| l.repo.clone())
                .into_iter()
                .collect()
        };
        match action {
            Action::StartAgent { options, .. } => options.chosen_repos(),
            Action::StartPrReview { review, .. }
            | Action::AddressReviewThreads { review, .. }
            | Action::FixFailedChecks { review, .. } => of_pr(review),
            _ => Vec::new(),
        }
    }

    /// First use of a repo: reads its default branch and the branch proposal
    /// of its `CONTRIBUTING.md` from GitHub and stores them as an entry of `[[agent.repos]]`
    /// (with `watch = false`, so the sync does not start loading all its pull requests). From
    /// then on the settings are the same ones every other part uses – and the user can edit them
    /// in the file. A repo that has settings already is left alone. Only reads from GitHub.
    pub(crate) async fn repo_settings_prepare(&self, action: &Action) -> Result<(), Msg> {
        for repo in self.action_repos(action) {
            self.repo_settings_prepare_one(&repo).await?;
        }
        Ok(())
    }

    async fn repo_settings_prepare_one(&self, repo: &str) -> Result<(), Msg> {
        let repo = repo.to_string();
        if !self.repo_usable(&repo) || self.repo_configured(&repo) {
            return Ok(());
        }
        let client = self.github_client_async().await?;
        let base = match self
            .repo_list_kept()
            .and_then(|l| l.repos.into_iter().find(|r| r.repo == repo))
            .map(|r| r.default_branch)
            .filter(|b| !b.is_empty())
        {
            Some(b) => b,
            None => {
                client
                    .repo_access(&repo)
                    .await
                    .map_err(|e| github_message(msg!(ServerAgentWhatRepoRead), &e))?
                    .default_branch
            }
        };
        // The proposal from the contribution guidelines; none is as good as the default pattern.
        let pattern = client
            .contributing(&repo)
            .await
            .ok()
            .flatten()
            .and_then(|(_, text)| cockpit_core::branch::suggest(&text))
            .map(|s| s.pattern)
            .filter(|p| cockpit_core::branch::validate(p).is_ok());
        let mut entry = AgentRepo::new(&repo);
        entry.watch = false;
        if agent_git::base_branch_ok(&base) {
            entry.base_branch = base;
        }
        if let Some(p) = pattern {
            entry.branch_pattern = p;
        }
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.repo_settings_save(entry))
            .await
            .map_err(|_| msg!(ServerAgentInternal))?;
        Ok(())
    }

    /// Adds an entry to `[[agent.repos]]` in `config.toml` and applies it. Without a
    /// configuration file (or if it cannot be written) the settings are only kept in memory until
    /// the restart; the failure is logged. Blocking.
    fn repo_settings_save(&self, entry: AgentRepo) {
        let cfg = self.agent_repos_config();
        let mut repos = cfg.repos.clone();
        // A default repo without its own entry keeps the usual settings next to the new one.
        if let Some(d) = &cfg.default
            && !repos.iter().any(|r| &r.repo == d)
        {
            repos.push(AgentRepo::new(d));
        }
        if repos.iter().any(|r| r.repo == entry.repo) {
            return;
        }
        repos.push(entry.clone());
        let change = ConfigChange {
            agent_repos: Some(AgentReposInput {
                repos: repos.clone(),
                default: cfg.default.clone(),
            }),
            ..ConfigChange::default()
        };
        if let Err((_, m)) = self.config_change(&change) {
            tracing::warn!(
                "Settings of {} not stored: {}",
                entry.repo,
                m.render(cockpit_i18n::Lang::En)
            );
            match self.inner.agent_repo_config.write() {
                Ok(mut c) => c.repos = repos,
                Err(v) => v.into_inner().repos = repos,
            }
        }
    }

    /// The repo a run works in: the one it was started for, or the default for runs of an
    /// old state without repo. Always one of the configured repos; otherwise the plain-text
    /// reason.
    pub(crate) fn run_repo(&self, run: &cockpit_core::AgentRun) -> Result<String, Msg> {
        let repos = self.agent_repos();
        let repo = if run.repo.is_empty() {
            repos.default.clone()
        } else {
            Some(run.repo.clone())
        };
        let Some(repo) = repo else {
            return Err(msg!(ServerAgentRepoNoneForRun));
        };
        if !self.repo_usable(&repo) {
            return Err(msg!(ServerAgentRepoNotAllowedAnyMore, repo = repo));
        }
        Ok(repo)
    }

    /// One repo of a run (`""` = the primary one). It must be a repo of the run.
    pub(crate) fn run_repo_of(
        &self,
        run: &cockpit_core::AgentRun,
        repo: &str,
    ) -> Result<String, Msg> {
        let primary = self.run_repo(run)?;
        if repo.is_empty() || repo == primary {
            return Ok(primary);
        }
        if run.repos.iter().any(|r| r.repo == repo) && self.repo_usable(repo) {
            return Ok(repo.to_string());
        }
        Err(msg!(ServerAgentRepoNotInRun, repo = repo))
    }

    /// The repos as configured, without the fallback to the sync repos (for the assistant).
    pub(crate) fn agent_repos_config(&self) -> AgentRepos {
        let cfg = match self.inner.agent_repo_config.read() {
            Ok(c) => c.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        AgentRepos {
            repos: cfg.repos,
            default: cfg.default,
        }
    }

    /// Settles the repo of a start before it is applied: the user's choice, otherwise the
    /// default repo. A start without either (no default, or several repos and none marked) is
    /// rejected with the reason: the user has to choose. Everything else passes unchanged.
    pub(crate) fn agent_route(&self, action: Action) -> Result<Action, Msg> {
        let repos = self.agent_repos();
        let (ticket, mut options) = match action {
            Action::StartAgent { ticket, options } => (ticket, options),
            // Follow-ups work on an existing branch: the repo of the pull request decides which
            // pattern that branch has to follow. The request's own values never count.
            Action::AddressReviewThreads {
                review,
                mut options,
            } => {
                self.pattern_for_pull_request(&repos, &review, &mut options);
                return Ok(Action::AddressReviewThreads { review, options });
            }
            Action::FixFailedChecks {
                review,
                mut options,
            } => {
                self.pattern_for_pull_request(&repos, &review, &mut options);
                return Ok(Action::FixFailedChecks { review, options });
            }
            // A review is written in the language the user picked, else the one detected from the
            // pull request, else the repo's: the repo's comes from the configuration only.
            Action::StartPrReview {
                review, language, ..
            } => {
                let repo = self
                    .inner
                    .live
                    .copy()
                    .sync
                    .pr(&review)
                    .map(|l| l.repo.clone())
                    .unwrap_or_default();
                let repo_language = self.agent_repos().find(&repo).map(|r| r.pr_language);
                return Ok(Action::StartPrReview {
                    review,
                    language,
                    repo_language,
                });
            }
            other => return Ok(other),
        };
        // Pattern and type come from the configuration and the ticket, never from the request.
        options.branch_pattern = DEFAULT_PATTERN.into();
        options.pr_language = cockpit_i18n::Lang::En;
        options.branch_type = BranchType::Feature.as_str().into();
        options.targets.clear();
        let chosen = options.chosen_repos();
        if !chosen.is_empty() {
            for repo in &chosen {
                if !self.repo_usable(repo) {
                    return Err(msg!(ServerAgentRepoNotAllowed, repo = repo));
                }
            }
            self.settle_targets(&repos, &chosen, &mut options);
        } else {
            // No choice: the default repo (`[agent] repo`, or the only configured one).
            let Some(default) = repos.default.clone() else {
                return Err(msg!(ServerAgentRepoNoDefault));
            };
            self.settle_targets(&repos, std::slice::from_ref(&default), &mut options);
        }
        if let Some(item) = self.inner.live.copy().item_by_key(&ticket) {
            options.branch_type = BranchType::of(item.issue_type.as_deref(), &item.labels)
                .as_str()
                .into();
        }
        Ok(Action::StartAgent { ticket, options })
    }

    /// Writes the chosen repos into the options with the base branch, pattern and language of
    /// each one from the configuration, whatever the request carried. The first repo is the
    /// primary one: its values also go into the common fields.
    fn settle_targets(&self, repos: &AgentRepos, chosen: &[String], options: &mut AgentOptions) {
        options.targets = chosen
            .iter()
            .map(|r| cockpit_core::RepoTarget {
                repo: r.clone(),
                base_branch: repos.base_branch(r),
                branch_pattern: repos.branch_pattern(r),
                pr_language: repos.pr_language(r),
            })
            .collect();
        if let Some(first) = options.targets.first().cloned() {
            options.base_branch = first.base_branch;
            options.branch_pattern = first.branch_pattern;
            options.pr_language = first.pr_language;
            options.repo = Some(first.repo);
        }
        options.repos = chosen.to_vec();
    }

    /// Sets base branch and pattern of a follow-up from the repo of the pull request.
    fn pattern_for_pull_request(
        &self,
        repos: &AgentRepos,
        review: &PrKey,
        options: &mut AgentOptions,
    ) {
        let ws = self.inner.live.copy();
        let repo = ws
            .sync
            .pr(review)
            .map(|l| l.repo.clone())
            .unwrap_or_default();
        options.base_branch = repos.base_branch(&repo);
        options.branch_pattern = repos.branch_pattern(&repo);
        options.pr_language = repos.pr_language(&repo);
    }
}
