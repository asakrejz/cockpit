//! Which repos the agent may work in and where a start request goes.

use super::*;
use cockpit_core::AgentOptions;
use cockpit_core::branch::{BranchType, DEFAULT_PATTERN};
use cockpit_core::model::PrKey;
use cockpit_i18n::{Msg, msg};

impl AppState {
    /// The repos the agent may work in, with the routing rules and the default.
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

    /// The repo a run works in: the one it was started for, or the default for runs of an
    /// old state without repo (or with the placeholder repo of the sample data). Always one
    /// of the configured repos; otherwise the plain-text reason.
    pub(crate) fn run_repo(&self, run: &cockpit_core::AgentRun) -> Result<String, Msg> {
        let repos = self.agent_repos();
        let repo = if run.repo.is_empty() || run.repo == cockpit_core::DEMO_REPO {
            repos.default.clone()
        } else {
            Some(run.repo.clone())
        };
        let Some(repo) = repo else {
            return Err(msg!(ServerAgentRepoNoneForRun));
        };
        if !repos.allows(&repo) {
            return Err(msg!(ServerAgentRepoNotAllowedAnyMore, repo = repo));
        }
        Ok(repo)
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

    /// Settles the repo of a start before it is applied: the user's choice if it is one of
    /// the configured repos, otherwise the one the routing rules name for the ticket's Jira
    /// project, otherwise the default. In real mode a start without any repo is rejected with
    /// the reason; the simulation keeps its own default. Everything else passes unchanged.
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
            other => return Ok(other),
        };
        // Pattern and type come from the configuration and the ticket, never from the request.
        options.branch_pattern = DEFAULT_PATTERN.into();
        options.branch_type = BranchType::Feature.as_str().into();
        if let Some(chosen) = options.repo.clone().filter(|r| !r.is_empty()) {
            if !repos.allows(&chosen) {
                return Err(msg!(ServerAgentRepoNotAllowed, repo = chosen));
            }
            // The base branch always belongs to the repo, whatever the request carries.
            options.base_branch = repos.base_branch(&chosen);
            options.branch_pattern = repos.branch_pattern(&chosen);
        } else {
            let ws = self.inner.live.copy();
            let project = ws
                .item_by_key(&ticket)
                .and_then(cockpit_core::Item::project)
                .map(str::to_string);
            match repos.choose(project.as_deref()) {
                RepoChoice::Rule(r) | RepoChoice::Default(r) => {
                    options.base_branch = repos.base_branch(&r);
                    options.branch_pattern = repos.branch_pattern(&r);
                    options.repo = Some(r);
                }
                RepoChoice::None if self.agent_real() => {
                    return Err(match project {
                        Some(p) => msg!(ServerAgentRepoNoRule, project = p),
                        None => msg!(ServerAgentRepoNoDefault),
                    });
                }
                RepoChoice::None => {}
            }
        }
        if let Some(item) = self.inner.live.copy().item_by_key(&ticket) {
            options.branch_type = BranchType::of(item.issue_type.as_deref(), &item.labels)
                .as_str()
                .into();
        }
        Ok(Action::StartAgent { ticket, options })
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
    }
}
