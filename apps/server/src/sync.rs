//! Jira and GitHub in the server: build services from configuration and .env file, run actions
//! first at the service and then apply locally, sync regularly.
//!
//! The core of the rules is in `cockpit-core` (`sync::merge`) and `cockpit-integrations`
//! (`outbound`); only the course runs here. A key is read fresh from the .env file for every use
//! (so `token set` works without restart), never stored or logged.

use std::time::Duration;

use cockpit_core::api::{
    ActionResponse, AssignableResponse, JiraProject, JiraProjectsResponse, SyncResponse,
};
use cockpit_core::{Action, ActionError, Actor, Source};
use cockpit_i18n::{Msg, msg};
use cockpit_integrations::github::{GitHubClient, PrLimits};
use cockpit_integrations::jira::{JiraClient, jql_for_selection};
use cockpit_integrations::oauth::{Endpoints, Service};
use cockpit_integrations::outbound::plan_call;
use cockpit_integrations::{GitHubService, JiraService, Services};

use cockpit_core::{AuthChoice, AuthKind};

use crate::app::{AppState, now_ms};
use crate::config::{
    AtlassianConfig, Config, DEFAULT_INTERVAL_S, GitHubConfig, GitHubProjectsConfig, JiraConfig,
    OAuthConfig,
};
use crate::live::Error as LiveError;
use crate::secrets;

/// How often an outdated fetch (a write happened during the fetch) is repeated.
const RETRIES: usize = 2;

/// Why an action was not applied.
#[derive(Debug)]
pub enum ActionFailure {
    /// The rules of the workspace reject it.
    Rule(ActionError),
    /// It cannot be mapped to the outside (e.g. no repo for branches).
    Plan(Msg),
    /// The service rejected the call or was not reachable. Nothing was changed.
    Service(Msg),
    Save(String),
    Internal,
}

impl AppState {
    pub(crate) async fn services(&self) -> Services {
        // First refresh (network), then build (.env file).
        self.oauth_refresh().await;
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.services_build())
            .await
            .unwrap_or_default()
    }

    pub(crate) async fn local_apply(
        &self,
        action: Action,
        actor: Actor,
    ) -> Result<ActionResponse, LiveError> {
        let s = self.clone();
        tokio::task::spawn_blocking(move || {
            s.inner.live.apply(&s.inner.db, action, actor, now_ms())
        })
        .await
        .unwrap_or_else(|_| Err(LiveError::Save("Internal error".into())))
    }

    /// Applies an action of the interface (as "Du"). If it acts to the outside, the
    /// call to the service goes first – **only when it confirms is it applied locally**. If
    /// it fails, nothing changes.
    pub async fn action_run(&self, action: Action) -> Result<ActionResponse, ActionFailure> {
        let inner = &self.inner;
        // The repo of a start is settled first (the user's choice, otherwise the default repo).
        self.repo_settings_prepare(&action)
            .await
            .map_err(ActionFailure::Plan)?;
        let action = self.agent_route(action).map_err(ActionFailure::Plan)?;
        // The agent only starts if everything is set up; otherwise the message says what is missing.
        let s = self.clone();
        let a = action.clone();
        let lock = tokio::task::spawn_blocking(move || s.agent_lock(&a))
            .await
            .map_err(|_| ActionFailure::Internal)?;
        if let Some(m) = lock {
            return Err(ActionFailure::Plan(m));
        }
        // The pull request has its own course (look for an existing one, create, then book).
        if matches!(action, Action::CreatePullRequest { .. }) {
            return self.pull_request_create(action).await;
        }
        // So do a new issue in a project and the conversion of a draft (several calls at GitHub).
        if matches!(
            action,
            Action::CreateIssue { .. } | Action::ConvertDraft { .. }
        ) {
            return self.project_issue_flow(action).await;
        }
        // A follow-up job only starts if GitHub has something for it to do.
        self.followup_precheck(&action).await?;
        let _lock = inner.lock.lock().await;

        // Probe on a copy: the rules (also "only Du may merge") apply before every call.
        let before = inner.live.copy();
        let mut afterwards = before.clone();
        afterwards
            .apply(action.clone(), Actor::User, now_ms())
            .map_err(ActionFailure::Rule)?;

        let mut outside = false;
        let services = self.services().await;
        if !services.is_empty() {
            let targets = services.targets(self.agent_repos().default);
            match plan_call(&before, &afterwards, &action, &targets) {
                Err(e) => return Err(ActionFailure::Plan(e.0)),
                Ok(None) => {}
                Ok(Some(call)) => {
                    services.execute(&call).await.map_err(|e| {
                        tracing::warn!("{}: call failed: {e}", call.service());
                        ActionFailure::Service(msg!(
                            ServerRunServiceFailed,
                            service = call.service().to_string(),
                            error = e.msg()
                        ))
                    })?;
                    inner
                        .written
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    outside = true;
                }
            }
        }
        // Which runs a removal takes out of the list (their leftovers are cleaned up afterwards).
        let removed: Vec<String> = if matches!(
            action,
            Action::RemoveAgentRun { .. } | Action::RemoveFinishedRuns
        ) {
            before
                .runs
                .iter()
                .filter(|r| !afterwards.runs.iter().any(|a| a.key() == r.key()))
                .map(|r| r.key())
                .collect()
        } else {
            Vec::new()
        };
        match self.local_apply(action, Actor::User).await {
            Ok(answer) => {
                if !removed.is_empty() {
                    let s = self.clone();
                    let _ = tokio::task::spawn_blocking(move || s.cleanup_removed(&removed)).await;
                }
                Ok(answer)
            }
            Err(LiveError::Action(e)) if outside => Err(ActionFailure::Service(msg!(
                ServerRunServiceDoneNotApplied,
                error = e.msg()
            ))),
            Err(LiveError::Action(e)) => Err(ActionFailure::Rule(e)),
            Err(LiveError::Save(m)) => Err(ActionFailure::Save(m)),
        }
    }

    /// A sync: fetch and apply as the action `SyncRemote` (author system). Pause sources in the
    /// quota limit. Was something written in the meantime? Then the fetch is
    /// outdated and is repeated.
    pub async fn sync(&self) -> Result<SyncResponse, ActionFailure> {
        self.sync_sources(&[Source::GitHub, Source::Jira, Source::GitHubProject])
            .await
    }

    /// Like [`AppState::sync`], but only asks the given sources (a webhook names one; the
    /// regular tick leaves out sources whose webhooks arrive and that were asked recently).
    pub async fn sync_sources(&self, only: &[Source]) -> Result<SyncResponse, ActionFailure> {
        let inner = &self.inner;
        let _only_one = inner.sync.lock().await;
        let services = self.services().await;
        if services.is_empty() {
            return Ok(SyncResponse {
                active: false,
                revision: inner.live.revision(),
                logged: 0,
            });
        }
        for attempt in 0..=RETRIES {
            let snapshot = inner.written.load(std::sync::atomic::Ordering::SeqCst);
            let now = now_ms();
            let known = inner.live.copy();
            let paused: Vec<Source> = [Source::GitHub, Source::Jira, Source::GitHubProject]
                .into_iter()
                .filter(|s| !only.contains(s) || known.sync.paused_until(*s, now).is_some())
                .collect();
            for s in only {
                inner.webhooks.fetched(*s, tokio::time::Instant::now());
            }
            let remote = services.fetch(&known, now, &paused).await;
            if remote.sources.is_empty() {
                return Ok(SyncResponse {
                    active: true,
                    revision: inner.live.revision(),
                    logged: 0,
                });
            }
            let _lock = inner.lock.lock().await;
            let outdated = inner.written.load(std::sync::atomic::Ordering::SeqCst) != snapshot;
            if outdated && attempt < RETRIES {
                continue;
            }
            // On the last attempt we take over the fetch anyway; the next tick evens it out.
            return match self
                .local_apply(Action::SyncRemote { remote }, Actor::System)
                .await
            {
                Ok(a) => Ok(SyncResponse {
                    active: true,
                    revision: a.revision,
                    logged: a.logged,
                }),
                Err(LiveError::Action(e)) => Err(ActionFailure::Rule(e)),
                Err(LiveError::Save(m)) => Err(ActionFailure::Save(m)),
            };
        }
        Err(ActionFailure::Internal)
    }
}

/// Starts the sync at the tick of the configuration (the first immediately).
pub fn sync_start(state: AppState) {
    let interval = state.sources().interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            // While a source's webhooks arrive it is only polled as a safety net.
            let due = state.sources_due();
            if !due.is_empty() {
                match state.sync_sources(&due).await {
                    Ok(a) if a.logged > 0 => {
                        tracing::info!("Sync: {} new entries in the log", a.logged)
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("Sync result not applied: {e:?}"),
                }
            }
            // Runs whose pull request is merged or closed are marked as done.
            state.reconcile_runs().await;
        }
    });
}

mod lookups;
mod services;
mod sources;
pub use sources::*;

#[cfg(test)]
mod tests;
