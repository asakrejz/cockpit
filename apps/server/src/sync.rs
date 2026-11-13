//! Jira und GitHub im Server: Dienste aus Konfiguration und Schlüsselbund bauen, Aktionen
//! erst beim Dienst ausführen und dann lokal anwenden, regelmäßig abgleichen.
//!
//! Der Kern der Regeln steht in `cockpit-core` (`sync::merge`) und `cockpit-integrations`
//! (`ausgehend`); hier läuft nur der Ablauf. Ein Schlüssel wird für jeden Gebrauch frisch aus
//! dem Schlüsselbund gelesen (so wirkt `token setzen` ohne Neustart), nie gespeichert oder geloggt.

use std::time::Duration;

use cockpit_core::api::{ActionResponse, JiraProject, JiraProjectsResponse, SyncResponse};
use cockpit_core::{Action, ActionError, Actor, Source};
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::{JiraClient, jql_for, jql_for_project};
use cockpit_integrations::oauth::{Endpoints, Service};
use cockpit_integrations::outbound::plane;
use cockpit_integrations::{GitHubService, JiraService, Services};

use crate::app::{AppState, now_ms};
use crate::config::{
    AtlassianConfig, Config, DEFAULT_INTERVAL_S, GitHubConfig, JiraConfig, OAuthConfig,
};
use crate::live::Error as LiveError;
use crate::secrets;

/// So oft wird ein veralteter Abruf (während des Abrufs wurde geschrieben) wiederholt.
const RETRIES: usize = 2;

/// Jira und GitHub, wie sie konfiguriert sind.
#[derive(Clone, Debug)]
pub struct Sources {
    pub github: Option<GitHubConfig>,
    pub jira: Option<JiraConfig>,
    /// Anmeldung per OAuth (nur Client-IDs; die Secrets liegen im Schlüsselbund).
    pub github_oauth: Option<OAuthConfig>,
    pub atlassian: Option<AtlassianConfig>,
    /// Adressen der OAuth-Dienste (Tests setzen nachgebaute ein).
    pub endpoints: Endpoints,
    pub interval: Duration,
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
        }
    }

    /// Ist irgendetwas eingerichtet, das (mit Schlüssel) laufen könnte?
    fn possible(&self) -> bool {
        self.github.as_ref().is_some_and(|g| !g.repos.is_empty()) || self.jira.is_some()
    }
}

/// Warum eine Aktion nicht angewendet wurde.
#[derive(Debug)]
pub enum ActionFailure {
    /// Die Regeln des Workspace lehnen sie ab.
    Rule(ActionError),
    /// Sie lässt sich nicht nach außen abbilden (z. B. kein Repo für Branches).
    Plan(String),
    /// Der Dienst hat den Aufruf abgelehnt oder war nicht erreichbar. Nichts wurde geändert.
    Service(String),
    Save(String),
    Intern,
}

impl AppState {
    /// Baut die Dienste, die eingerichtet sind **und** einen Schlüssel haben. Blockierend
    /// (Schlüsselbund).
    pub(crate) fn services_build(&self) -> Services {
        let inner = &self.inner;
        let sources = self.sources();
        let mut services = Services::default();
        if let Some(g) = sources.github.as_ref().filter(|g| !g.repos.is_empty()) {
            // Die Anmeldung per OAuth geht vor dem Token.
            let access = match self.oauth_tokens(Service::GitHub) {
                Some(t) => Some(t.access),
                None => self.token(secrets::GITHUB),
            };
            if let Some(t) = access {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        services.github = Some(GitHubService {
                            client,
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
            // Ein in der App gewähltes Projekt hat Vorrang vor `projects` und `jql` der Datei.
            let chosen = inner.live.copy().jira_project;
            let in_project = chosen.is_some();
            let jql = match &chosen {
                Some(p) => jql_for_project(p),
                None => jql_for(&j.projects, j.jql.as_deref()),
            };
            match (client, jql) {
                (Ok(client), Ok(jql)) => {
                    services.jira = Some(JiraService {
                        client,
                        jql,
                        in_project,
                    })
                }
                (Err(e), _) | (_, Err(e)) => tracing::warn!("Jira: {e}"),
            }
        }
        services
    }

    /// Der Jira-Client: per OAuth (über das Atlassian-Gateway), sonst mit E-Mail und API-Token.
    /// `None`, wenn Jira nicht eingerichtet ist oder der Zugang fehlt. Blockierend.
    pub(crate) fn jira_client(&self) -> Option<Result<JiraClient, cockpit_integrations::Error>> {
        if let Some(c) = self.jira_oauth_client() {
            return Some(c);
        }
        let sources = self.sources();
        let j = sources.jira.as_ref().filter(|j| !j.site.is_empty())?;
        let t = self.token(secrets::JIRA)?;
        Some(JiraClient::new(&j.site, &j.email, t))
    }

    /// Der Zugang zu GitHub: die Anmeldung per OAuth geht vor dem Token. Blockierend.
    pub(crate) fn github_access(&self) -> Option<cockpit_integrations::Secret> {
        match self.oauth_tokens(Service::GitHub) {
            Some(t) => Some(t.access),
            None => self.token(secrets::GITHUB),
        }
    }

    /// Ein Schlüssel aus dem Schlüsselbund, leer zählt als fehlend. Blockierend.
    pub(crate) fn token(&self, name: &str) -> Option<cockpit_integrations::Secret> {
        match self.inner.secrets.get(name) {
            Ok(t) => t.filter(|t| !t.is_empty()),
            Err(e) => {
                tracing::warn!("Schlüssel „{name}“ nicht lesbar: {e}");
                None
            }
        }
    }

    /// Die Jira-Projekte, die der Schlüssel sehen darf – für die Auswahl in der Inbox.
    /// Ohne Jira oder Schlüssel: `aktiv: false`.
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
        let client = client.map_err(|e| ActionFailure::Plan(format!("Jira: {e}")))?;
        let projects = client.projects().await.map_err(|e| {
            tracing::warn!("Jira-Projekte: {e}");
            ActionFailure::Service(format!("Jira liefert keine Projekte: {e}"))
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
        // Zuerst auffrischen (Netzwerk), dann bauen (Schlüsselbund).
        self.oauth_refresh().await;
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.services_build())
            .await
            .unwrap_or_default()
    }

    async fn local_apply(&self, action: Action, actor: Actor) -> Result<ActionResponse, LiveError> {
        let s = self.clone();
        tokio::task::spawn_blocking(move || {
            s.inner.live.apply(&s.inner.db, action, actor, now_ms())
        })
        .await
        .unwrap_or_else(|_| Err(LiveError::Save("Interner Fehler".into())))
    }

    /// Wendet eine Aktion der Oberfläche an (als „Du“). Wirkt sie nach außen, geht erst der
    /// Aufruf an den Dienst – **erst wenn er bestätigt, wird sie lokal angewendet**. Scheitert
    /// er, ändert sich nichts.
    pub async fn action_run(&self, action: Action) -> Result<ActionResponse, ActionFailure> {
        let inner = &self.inner;
        // Der echte Agent startet nur, wenn alles eingerichtet ist; sonst sagt die Meldung, was fehlt.
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
        let _lock = inner.lock.lock().await;

        // Probe auf einer Kopie: Die Regeln (auch „nur Du darfst mergen“) gelten vor jedem Aufruf.
        let before = inner.live.copy();
        let mut afterwards = before.clone();
        afterwards
            .apply(action.clone(), Actor::User, now_ms())
            .map_err(ActionFailure::Rule)?;

        let mut outside = false;
        let services = self.services().await;
        if !services.is_empty() {
            match plane(&before, &afterwards, &action, &services.targets()) {
                Err(e) => return Err(ActionFailure::Plan(e.0)),
                Ok(None) => {}
                Ok(Some(call)) => {
                    services.execute(&call).await.map_err(|e| {
                        tracing::warn!("{}: Aufruf gescheitert: {e}", call.service());
                        ActionFailure::Service(format!(
                            "{} hat die Aktion nicht ausgeführt: {e}. Es wurde nichts geändert.",
                            call.service()
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
            Err(LiveError::Action(e)) if outside => Err(ActionFailure::Service(format!(
                "Beim Dienst ausgeführt, lokal aber nicht übernommen ({e}). Der nächste Abgleich holt es nach."
            ))),
            Err(LiveError::Action(e)) => Err(ActionFailure::Rule(e)),
            Err(LiveError::Save(m)) => Err(ActionFailure::Save(m)),
        }
    }

    /// Ein Abgleich: abrufen und als Aktion `SyncRemote` (Urheber System) anwenden. Quellen im
    /// Kontingent-Limit pausieren. Geschrieben wurde währenddessen? Dann ist der Abruf
    /// veraltet und wird wiederholt.
    pub async fn sync(&self) -> Result<SyncResponse, ActionFailure> {
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
                .filter(|s| known.sync.paused_until(*s, now).is_some())
                .collect();
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
            // Beim letzten Versuch übernehmen wir den Abruf trotzdem; der nächste Takt gleicht aus.
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

/// Startet den Abgleich im Takt der Konfiguration (der erste sofort).
pub fn sync_start(state: AppState) {
    let interval = state.sources().interval;
    tokio::spawn(async move {
        let mut takt = tokio::time::interval(interval);
        takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            takt.tick().await;
            match state.sync().await {
                Ok(a) if a.logged > 0 => {
                    tracing::info!("Abgleich: {} neue Einträge im Protokoll", a.logged)
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Abgleich nicht übernommen: {e:?}"),
            }
        }
    });
}

#[cfg(test)]
mod tests;
