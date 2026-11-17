//! Which repos the agent may work in and where a start request goes.

use super::*;

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
    pub(crate) fn run_repo(&self, run: &cockpit_core::AgentRun) -> Result<String, String> {
        let repos = self.agent_repos();
        let repo = if run.repo.is_empty() || run.repo == cockpit_core::DEMO_REPO {
            repos.default.clone()
        } else {
            Some(run.repo.clone())
        };
        let Some(repo) = repo else {
            return Err(
                "Für diesen Lauf ist kein Repo festgelegt. Starte ihn neu und wähle ein Repo aus."
                    .into(),
            );
        };
        if !repos.allows(&repo) {
            return Err(format!(
                "Das Repo „{repo}“ ist für den Agenten nicht (mehr) freigegeben. Trage es unter „Einrichten“ › „GitHub“ ein."
            ));
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
    pub(crate) fn agent_route(&self, action: Action) -> Result<Action, String> {
        let Action::StartAgent {
            ticket,
            mut options,
        } = action
        else {
            return Ok(action);
        };
        let repos = self.agent_repos();
        if let Some(chosen) = options.repo.clone().filter(|r| !r.is_empty()) {
            if !repos.allows(&chosen) {
                return Err(format!(
                    "Das Repo „{chosen}“ ist für den Agenten nicht freigegeben. Trage es unter „Einrichten“ › „GitHub“ ein."
                ));
            }
        } else {
            let ws = self.inner.live.copy();
            let project = ws
                .item_by_key(&ticket)
                .and_then(cockpit_core::Item::project)
                .map(str::to_string);
            match repos.choose(project.as_deref()) {
                RepoChoice::Rule(r) | RepoChoice::Default(r) => {
                    options.base_branch = repos.base_branch(&r);
                    options.repo = Some(r);
                }
                RepoChoice::None if self.agent_real() => {
                    return Err(match project {
                        Some(p) => format!(
                            "Für das Jira-Projekt {p} ist kein Repo zugeordnet und es gibt kein Standard-Repo. Wähle vor dem Start ein Repo aus."
                        ),
                        None => {
                            "Es gibt kein Standard-Repo. Wähle vor dem Start ein Repo aus.".into()
                        }
                    });
                }
                RepoChoice::None => {}
            }
        }
        Ok(Action::StartAgent { ticket, options })
    }
}
