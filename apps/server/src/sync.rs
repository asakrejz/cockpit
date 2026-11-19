//! Jira and GitHub in the server: build services from configuration and .env file, run actions
//! first at the service and then apply locally, sync regularly.
//!
//! The core of the rules is in `cockpit-core` (`sync::merge`) and `cockpit-integrations`
//! (`outbound`); only the course runs here. A key is read fresh from the .env file for every use
//! (so `token set` works without restart), never stored or logged.

use std::time::Duration;

use cockpit_core::api::{ActionResponse, JiraProject, JiraProjectsResponse, SyncResponse};
use cockpit_core::{Action, ActionError, Actor, Source};
use cockpit_i18n::{Msg, msg};
use cockpit_integrations::github::{GitHubClient, PrLimits};
use cockpit_integrations::jira::{JiraClient, jql_for_selection};
use cockpit_integrations::oauth::{Endpoints, Service};
use cockpit_integrations::outbound::plane;
use cockpit_integrations::{GitHubService, JiraService, Services};

use crate::app::{AppState, now_ms};
use crate::config::{
    AtlassianConfig, Config, DEFAULT_INTERVAL_S, GitHubConfig, JiraConfig, OAuthConfig,
};
use crate::live::Error as LiveError;
use crate::secrets;

/// This often an outdated fetch (a write happened during the fetch) is repeated.
const RETRIES: usize = 2;

/// Jira and GitHub as configured.
#[derive(Clone, Debug)]
pub struct Sources {
    pub github: Option<GitHubConfig>,
    pub jira: Option<JiraConfig>,
    /// Sign-in via OAuth (client IDs only; the secrets are in the .env file).
    pub github_oauth: Option<OAuthConfig>,
    pub atlassian: Option<AtlassianConfig>,
    /// Addresses of the OAuth services (tests insert imitations).
    pub endpoints: Endpoints,
    pub interval: Duration,
    /// How many open pull requests the GitHub sync loads.
    pub pr_limits: PrLimits,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            github: None,
            jira: None,
            github_oauth: None,
            atlassian: None,
            endpoints: Endpoints::default(),
            interval: Duration::from_secs(DEFAULT_INTERVAL_S),
            pr_limits: PrLimits::default(),
        }
    }
}

impl Sources {
    pub fn from_config(config: &Config) -> Self {
        Self {
            github: config.github.clone(),
            jira: config.jira.clone(),
            github_oauth: config.github_oauth.clone(),
            atlassian: config.atlassian.clone(),
            endpoints: Endpoints::default(),
            interval: Duration::from_secs(config.interval_s),
            pr_limits: config.github_pr_limits,
        }
    }

    /// Is anything set up that could run (with a key)?
    fn possible(&self) -> bool {
        self.github.as_ref().is_some_and(|g| !g.repos.is_empty()) || self.jira.is_some()
    }
}

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
    Intern,
}

impl AppState {
    /// Builds the services that are set up **and** have a key. Blocking
    /// (.env file).
    pub(crate) fn services_build(&self) -> Services {
        let inner = &self.inner;
        let sources = self.sources();
        let mut services = Services::default();
        if let Some(g) = sources.github.as_ref().filter(|g| !g.repos.is_empty()) {
            // The sign-in via OAuth goes before the token.
            let access = match self.oauth_tokens(Service::GitHub) {
                Some(t) => Some(t.access),
                None => self.token(secrets::GITHUB),
            };
            if let Some(t) = access {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        services.github = Some(GitHubService {
                            client: client.with_limits(sources.pr_limits),
                            repos: g.repos.clone(),
                        })
                    }
                    Err(e) => tracing::warn!("GitHub: {e}"),
                }
            }
        }
        if let Some(j) = &sources.jira
            && let Some(client) = self.jira_client()
        {
            // A project chosen in the app takes precedence over `projects` and `jql` of the file.
            let live = inner.live.copy();
            let query = jql_for_selection(
                &j.projects,
                j.jql.as_deref(),
                live.jira_project.as_deref(),
                live.inbox_filter.sprint,
            );
            match (client, query) {
                (Ok(client), Ok(query)) => {
                    services.jira = Some(JiraService {
                        client,
                        jql: query.jql,
                        in_project: query.in_selection,
                        fields: cockpit_integrations::jira::FieldPrefs {
                            hidden: j.hidden_fields.clone(),
                            order: j.field_order.clone(),
                        },
                    })
                }
                (Err(e), _) | (_, Err(e)) => tracing::warn!("Jira: {e}"),
            }
        }
        services
    }

    /// The Jira client: via OAuth (through the Atlassian gateway), otherwise with email and API token.
    /// `None` if Jira is not set up or the access is missing. Blocking.
    pub(crate) fn jira_client(&self) -> Option<Result<JiraClient, cockpit_integrations::Error>> {
        if let Some(c) = self.jira_oauth_client() {
            return Some(c);
        }
        let sources = self.sources();
        let j = sources.jira.as_ref().filter(|j| !j.site.is_empty())?;
        let t = self.token(secrets::JIRA)?;
        Some(JiraClient::new(&j.site, &j.email, t))
    }

    /// The access to GitHub: the sign-in via OAuth goes before the token. Blocking.
    pub(crate) fn github_access(&self) -> Option<cockpit_integrations::Secret> {
        match self.oauth_tokens(Service::GitHub) {
            Some(t) => Some(t.access),
            None => self.token(secrets::GITHUB),
        }
    }

    /// A key from the .env file, empty counts as missing. Blocking.
    pub(crate) fn token(&self, name: &str) -> Option<cockpit_integrations::Secret> {
        match self.inner.secrets.get(name) {
            Ok(t) => t.filter(|t| !t.is_empty()),
            Err(e) => {
                tracing::warn!("Key \"{name}\" not readable: {e}");
                None
            }
        }
    }

    /// The Jira projects that the key may see – for the selection in the inbox.
    /// Without Jira or key: `active: false`.
    pub async fn jira_projects(&self) -> Result<JiraProjectsResponse, ActionFailure> {
        if self.sources().jira.is_none() {
            return Ok(JiraProjectsResponse::default());
        }
        self.oauth_refresh().await;
        let s = self.clone();
        let client = tokio::task::spawn_blocking(move || s.jira_client())
            .await
            .map_err(|_| ActionFailure::Intern)?;
        let Some(client) = client else {
            return Ok(JiraProjectsResponse::default());
        };
        let client = client.map_err(|e| {
            ActionFailure::Plan(msg!(ServerRunJiraError, error = Msg::raw(e.to_string())))
        })?;
        let projects = client.projects().await.map_err(|e| {
            tracing::warn!("Jira projects: {e}");
            ActionFailure::Service(msg!(
                ServerRunJiraNoProjects,
                error = Msg::raw(e.to_string())
            ))
        })?;
        Ok(JiraProjectsResponse {
            active: true,
            projects: projects
                .into_iter()
                .map(|p| JiraProject {
                    key: p.key,
                    name: p.name,
                })
                .collect(),
        })
    }

    async fn services(&self) -> Services {
        if !self.sources().possible() {
            return Services::default();
        }
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
        // The repo of a start is settled first (user's choice or routing rules).
        let action = self.agent_route(action).map_err(ActionFailure::Plan)?;
        // The real agent only starts if everything is set up; otherwise the message says what is missing.
        if self.agent_real() {
            let s = self.clone();
            let a = action.clone();
            let lock = tokio::task::spawn_blocking(move || s.agent_lock(&a))
                .await
                .map_err(|_| ActionFailure::Intern)?;
            if let Some(m) = lock {
                return Err(ActionFailure::Plan(m));
            }
        }
        // The pull request has its own course (look for an existing one, create, then book).
        if matches!(action, Action::CreatePullRequest { .. }) {
            return self.pull_request_create(action).await;
        }
        // A follow-up job only starts if GitHub has something for it to do.
        if self.agent_real() {
            self.followup_precheck(&action).await?;
        }
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
            match plane(&before, &afterwards, &action, &services.targets()) {
                Err(e) => return Err(ActionFailure::Plan(Msg::raw(e.0))),
                Ok(None) => {}
                Ok(Some(call)) => {
                    services.execute(&call).await.map_err(|e| {
                        tracing::warn!("{}: call failed: {e}", call.service());
                        ActionFailure::Service(msg!(
                            ServerRunServiceFailed,
                            service = call.service().to_string(),
                            error = Msg::raw(e.to_string())
                        ))
                    })?;
                    inner
                        .written
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    outside = true;
                }
            }
        }
        match self.local_apply(action, Actor::User).await {
            Ok(answer) => Ok(answer),
            Err(LiveError::Action(e)) if outside => Err(ActionFailure::Service(msg!(
                ServerRunServiceDoneNotApplied,
                error = Msg::raw(e.to_string())
            ))),
            Err(LiveError::Action(e)) => Err(ActionFailure::Rule(e)),
            Err(LiveError::Save(m)) => Err(ActionFailure::Save(m)),
        }
    }

    /// A sync: fetch and apply as the action `SyncRemote` (author system). Pause sources in the
    /// quota limit. Was something written in the meantime? Then the fetch is
    /// outdated and is repeated.
    pub async fn sync(&self) -> Result<SyncResponse, ActionFailure> {
        self.sync_sources(&[Source::GitHub, Source::Jira]).await
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
            let paused: Vec<Source> = [Source::GitHub, Source::Jira]
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
        Err(ActionFailure::Intern)
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

#[cfg(test)]
mod tests;
