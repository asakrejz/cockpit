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

/// This often an outdated fetch (a write happened during the fetch) is repeated.
const RETRIES: usize = 2;

/// Jira and GitHub as configured.
#[derive(Clone, Debug)]
pub struct Sources {
    pub github: Option<GitHubConfig>,
    pub jira: Option<JiraConfig>,
    /// GitHub Projects as the source of tickets.
    pub github_projects: Option<GitHubProjectsConfig>,
    /// The picked sign-in method per service (`None`: OAuth first, then token).
    pub github_auth: Option<cockpit_core::AuthChoice>,
    pub jira_auth: Option<cockpit_core::AuthChoice>,
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
            github_projects: None,
            github_auth: None,
            jira_auth: None,
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
            github_projects: config.github_projects.clone(),
            github_auth: config.github_auth,
            jira_auth: config.jira_auth,
            github_oauth: config.github_oauth.clone(),
            atlassian: config.atlassian.clone(),
            endpoints: Endpoints::default(),
            interval: Duration::from_secs(config.interval_s),
            pr_limits: config.github_pr_limits,
        }
    }

    /// The GitHub settings; without a `[github]` section the defaults (address of github.com, no
    /// watched repos) apply, because the sync needs no repo list.
    pub fn github_config(&self) -> GitHubConfig {
        self.github.clone().unwrap_or_default()
    }

    /// Where tickets come from (Jira alone unless `[github_projects]` says otherwise).
    pub fn tickets_from(&self) -> cockpit_core::TicketsFrom {
        self.github_projects
            .as_ref()
            .map_or(cockpit_core::TicketsFrom::Jira, |p| p.tickets_from)
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
    Internal,
}

impl AppState {
    /// Builds the services that are set up **and** have a key. Blocking
    /// (.env file).
    pub(crate) fn services_build(&self) -> Services {
        let inner = &self.inner;
        let sources = self.sources();
        let mut services = Services::default();
        {
            let g = sources.github_config();
            // The sign-in via OAuth goes before the token.
            if let Some((t, app)) = self.github_credentials() {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        services.github = Some(GitHubService {
                            client: client.with_limits(sources.pr_limits).with_app(app),
                            repos: g.repos.clone(),
                        })
                    }
                    Err(e) => tracing::warn!("GitHub: {e}"),
                }
            }
            if let (Some(p), Some((t, app))) = (&sources.github_projects, self.github_credentials())
                && p.tickets_from.uses_projects()
                && !p.projects.is_empty()
            {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        services.projects = Some(cockpit_integrations::ProjectsService {
                            client: client.with_app(app),
                            projects: p.projects.clone(),
                            status_field: p.status_field.clone(),
                            team_field: p.team_field.clone(),
                        })
                    }
                    Err(e) => tracing::warn!("GitHub Projects: {e}"),
                }
            }
        }
        if let Some(j) = sources
            .jira
            .as_ref()
            .filter(|_| sources.tickets_from().uses_jira())
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
                        client: client.with_team_field(j.team_field.as_deref()),
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
        let sources = self.sources();
        let token = sources
            .jira
            .as_ref()
            .filter(|j| !j.site.is_empty())
            .and_then(|j| self.token(secrets::JIRA).map(|t| (j, t)));
        let kind = AuthChoice::pick(
            sources.jira_auth,
            token.is_some(),
            self.oauth_state(Service::Atlassian),
        );
        match kind {
            AuthKind::OAuth => self.jira_oauth_client(),
            AuthKind::Token => token.map(|(j, t)| JiraClient::new(&j.site, &j.email, t)),
            AuthKind::None => None,
        }
    }

    /// The access to GitHub and whether it is the sign-in via the app. The picked method decides
    /// ([`cockpit_core::AuthChoice::pick`], the same rule as
    /// [`cockpit_core::ServiceStatus::resolve`]); without a choice a working sign-in goes before
    /// the token. Blocking.
    pub(crate) fn github_credentials(&self) -> Option<(cockpit_integrations::Secret, bool)> {
        let token = self.token(secrets::GITHUB);
        let kind = AuthChoice::pick(
            self.sources().github_auth,
            token.is_some(),
            self.oauth_state(Service::GitHub),
        );
        match kind {
            AuthKind::OAuth => self.oauth_tokens(Service::GitHub).map(|t| (t.access, true)),
            AuthKind::Token => token.map(|t| (t, false)),
            AuthKind::None => None,
        }
    }

    /// The access to GitHub (see [`Self::github_credentials`]). Blocking.
    pub(crate) fn github_access(&self) -> Option<cockpit_integrations::Secret> {
        self.github_credentials().map(|(t, _)| t)
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
            .map_err(|_| ActionFailure::Internal)?;
        let Some(client) = client else {
            return Ok(JiraProjectsResponse::default());
        };
        let client =
            client.map_err(|e| ActionFailure::Plan(msg!(ServerRunJiraError, error = e.msg())))?;
        let projects = client.projects().await.map_err(|e| {
            tracing::warn!("Jira projects: {e}");
            ActionFailure::Service(msg!(ServerRunJiraNoProjects, error = e.msg()))
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

    /// People Jira lets you assign tickets of `project` to, whose name matches `query`.
    /// Read only. Without Jira or key: `active: false`.
    pub async fn jira_assignable(
        &self,
        project: &str,
        query: &str,
    ) -> Result<AssignableResponse, ActionFailure> {
        if self.sources().jira.is_none() {
            return Ok(AssignableResponse::default());
        }
        self.oauth_refresh().await;
        let s = self.clone();
        let client = tokio::task::spawn_blocking(move || s.jira_client())
            .await
            .map_err(|_| ActionFailure::Internal)?;
        let Some(client) = client else {
            return Ok(AssignableResponse::default());
        };
        let client =
            client.map_err(|e| ActionFailure::Plan(msg!(ServerRunJiraError, error = e.msg())))?;
        let people = client
            .assignable_people(project, query)
            .await
            .map_err(|e| {
                tracing::warn!("Jira assignable users: {e}");
                ActionFailure::Service(msg!(ServerRunJiraNoPeople, error = e.msg()))
            })?;
        Ok(AssignableResponse {
            active: true,
            people,
        })
    }

    /// People GitHub lets you assign issues of `repo` to, whose login matches `query`. Read only.
    /// Without GitHub or key: `active: false`.
    pub async fn github_assignable(
        &self,
        repo: &str,
        query: &str,
    ) -> Result<AssignableResponse, ActionFailure> {
        self.oauth_refresh().await;
        let s = self.clone();
        let credentials = tokio::task::spawn_blocking(move || s.github_credentials())
            .await
            .map_err(|_| ActionFailure::Internal)?;
        let Some((token, app)) = credentials else {
            return Ok(AssignableResponse::default());
        };
        let client = GitHubClient::new(&self.sources().github_config().api_url, token)
            .map_err(|e| ActionFailure::Plan(msg!(ServerRunGithubError, error = e.msg())))?
            .with_app(app);
        let me = client.me().await.unwrap_or_default();
        let logins = client.assignees(repo, query).await.map_err(|e| {
            tracing::warn!("GitHub assignees: {e}");
            ActionFailure::Service(msg!(ServerRunGithubNoPeople, error = e.msg()))
        })?;
        Ok(AssignableResponse {
            active: true,
            people: logins
                .into_iter()
                .map(|l| {
                    if !me.is_empty() && l.eq_ignore_ascii_case(&me) {
                        cockpit_core::Person::me()
                    } else {
                        cockpit_core::Person::new(&l, &cockpit_integrations::text::initials(&l))
                    }
                })
                .collect(),
        })
    }

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

#[cfg(test)]
mod tests;
