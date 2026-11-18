//! What a merged or closed pull request leaves behind: marking its run „erledigt“, and deleting
//! the branch of the agent – the latter only on the user's click.
//!
//! * **Reconcile** (read-only against GitHub): the pull request of a quiet run is looked up; if it
//!   is merged or closed, the run gets `Action::CloseAgentRun` (system) and its clone is removed.
//! * **Branch delete:** exactly one branch, only after `cockpit_core::branch::deletable` agreed
//!   with the facts read from GitHub *at that moment* (pull request state and head, default
//!   branch, protection). Never automatic, never another branch, never a force of any kind.

use cockpit_core::api::{BranchCheck, BranchDeleted};
use cockpit_core::branch::{DeleteFacts, deletable_msg};
use cockpit_core::{Action, Actor, AgentRun, Closure, RunKind};
use cockpit_integrations::Error as ServiceError;
use cockpit_integrations::github::GitHubClient;

use super::*;
use crate::agent_followup::github_message;
use crate::sync::ActionFailure;

/// At most this many pull requests are looked up per pass.
const MAX_LOOKUPS: usize = 20;

/// How a pull request stands, from the answer of GitHub.
fn ended(pull: &cockpit_integrations::github::dto::Pull) -> Option<Closure> {
    if pull.merged {
        Some(Closure::Merged)
    } else if pull.state == "closed" {
        Some(Closure::Closed)
    } else {
        None
    }
}

/// The run of an implementation whose branch the user may delete.
fn own_branch_run(run: &AgentRun) -> Result<u32, String> {
    match (&run.kind, &run.pull_request) {
        (RunKind::Implement, Some(p)) if run.real => Ok(p.number),
        _ => Err("Zu diesem Lauf gehört kein Branch, den das Cockpit angelegt hat.".into()),
    }
}

struct Plan {
    client: GitHubClient,
    repo: String,
    branch: String,
    verdict: Result<(), cockpit_i18n::Msg>,
}

impl AppState {
    /// Looks up the pull requests of quiet runs and marks the runs whose pull request is merged or
    /// closed as „erledigt“; their clone goes away. Reads from GitHub, changes nothing there.
    pub(crate) async fn reconcile_runs(&self) {
        let watch = self.inner.live.copy().runs_to_watch();
        if watch.is_empty() {
            return;
        }
        let Ok(client) = self.github_client_async().await else {
            return;
        };
        for (key, pr) in watch.into_iter().take(MAX_LOOKUPS) {
            let Ok(pull) = client.pull(&pr.repo, pr.number).await else {
                continue;
            };
            let Some(closure) = ended(&pull) else {
                continue;
            };
            let action = Action::CloseAgentRun {
                run: key.clone(),
                closure,
            };
            match self.local_apply(action, Actor::System).await {
                Ok(_) => {
                    let dir = self.agent_dir().join("runs").join(agent_run::run_dir(&key));
                    let _ = tokio::fs::remove_dir_all(dir).await;
                }
                Err(e) => tracing::debug!("Run {key} not marked as done: {e:?}"),
            }
        }
    }

    async fn branch_plan(&self, key: &str) -> Result<Plan, String> {
        let ws = self.inner.live.copy();
        let run = ws
            .runs
            .iter()
            .find(|r| r.key() == key)
            .ok_or("Diesen Lauf gibt es nicht.")?;
        let number = own_branch_run(run)?;
        let repo = self.run_repo(run)?;
        let branch = run.branch();
        let repos = self.agent_repos();
        let (pattern, base) = (repos.branch_pattern(&repo), repos.base_branch(&repo));
        let client = self.github_client_async().await?;
        let read = |what: &str, e: ServiceError| github_message(what, &e);
        let pull = client
            .pull(&repo, number)
            .await
            .map_err(|e| read("GitHub liefert den Pull Request nicht", e))?;
        let default_branch = client
            .repo_access(&repo)
            .await
            .map_err(|e| read("GitHub liefert das Repo nicht", e))?
            .default_branch;
        let verdict = match client.branch_protected(&repo, &branch).await {
            Err(ServiceError::NotFound) => Err(cockpit_i18n::Msg::raw(format!(
                "Den Branch „{branch}“ gibt es auf GitHub nicht mehr."
            ))),
            Err(e) => return Err(read("GitHub liefert den Branch nicht", e)),
            Ok(protected) => deletable_msg(&DeleteFacts {
                pattern: &pattern,
                base_branch: &base,
                default_branch: &default_branch,
                branch: &branch,
                pull_head: &pull.head.name,
                run_branch: &branch,
                protected,
                ended: ended(&pull),
            }),
        };
        Ok(Plan {
            client,
            repo,
            branch,
            verdict,
        })
    }

    /// `GET /api/agent/branch?run=`: may the branch be deleted? Reads only.
    pub(crate) async fn branch_check(&self, key: &str) -> BranchCheck {
        match self.branch_plan(key).await {
            Ok(p) => BranchCheck {
                branch: p.branch,
                allowed: p.verdict.is_ok(),
                reason: p.verdict.clone().err().map(|m| m.de()),
                reason_msg: p.verdict.err(),
            },
            Err(reason) => BranchCheck {
                branch: self
                    .inner
                    .live
                    .copy()
                    .runs
                    .iter()
                    .find(|r| r.key() == key)
                    .map(AgentRun::branch)
                    .unwrap_or_default(),
                allowed: false,
                reason: Some(reason),
                reason_msg: None,
            },
        }
    }

    /// `POST /api/agent/branch`: deletes the branch of the run – after checking everything again.
    pub(crate) async fn branch_delete(&self, key: &str) -> Result<BranchDeleted, ActionFailure> {
        let plan = self.branch_plan(key).await.map_err(ActionFailure::Plan)?;
        plan.verdict.map_err(|m| ActionFailure::Plan(m.de()))?;
        plan.client
            .branch_delete(&plan.repo, &plan.branch)
            .await
            .map_err(|e| {
                ActionFailure::Service(github_message("GitHub löscht den Branch nicht", &e))
            })?;
        tracing::info!(
            "Branch {} of {} deleted on the user's click",
            plan.branch,
            plan.repo
        );
        Ok(BranchDeleted {
            branch: plan.branch,
        })
    }
}
