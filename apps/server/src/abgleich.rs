//! Jira und GitHub im Server: Dienste aus Konfiguration und .env-Datei bauen, Aktionen
//! erst beim Dienst ausführen und dann lokal anwenden, regelmäßig abgleichen.
//!
//! Der Kern der Regeln steht in `cockpit-core` (`sync::merge`) und `cockpit-integrations`
//! (`ausgehend`); hier läuft nur der Ablauf. Ein Schlüssel wird für jeden Gebrauch frisch aus
//! der .env-Datei gelesen (so wirkt `token setzen` ohne Neustart), nie gespeichert oder geloggt.

use std::time::Duration;

use cockpit_core::api::{ActionResponse, JiraProject, JiraProjectsResponse, SyncResponse};
use cockpit_core::{Action, ActionError, Actor, Source};
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::{JiraClient, jql_for, jql_for_project};
use cockpit_integrations::oauth::{Endpoints, Service};
use cockpit_integrations::outbound::plane;
use cockpit_integrations::{GitHubService, JiraService, Services};

use crate::app::{AppState, jetzt_ms};
use crate::config::{
    AtlassianKonfig, Config, GitHubKonfig, JiraKonfig, OAuthKonfig, STANDARD_INTERVALL_S,
};
use crate::live::Fehler as LiveFehler;
use crate::secrets;

/// So oft wird ein veralteter Abruf (während des Abrufs wurde geschrieben) wiederholt.
const WIEDERHOLUNGEN: usize = 2;

/// Jira und GitHub, wie sie konfiguriert sind.
#[derive(Clone, Debug)]
pub struct Quellen {
    pub github: Option<GitHubKonfig>,
    pub jira: Option<JiraKonfig>,
    /// Anmeldung per OAuth (nur Client-IDs; die Secrets liegen in der .env-Datei).
    pub github_oauth: Option<OAuthKonfig>,
    pub atlassian: Option<AtlassianKonfig>,
    /// Adressen der OAuth-Dienste (Tests setzen nachgebaute ein).
    pub endpunkte: Endpoints,
    pub intervall: Duration,
}

impl Default for Quellen {
    fn default() -> Self {
        Self {
            github: None,
            jira: None,
            github_oauth: None,
            atlassian: None,
            endpunkte: Endpoints::default(),
            intervall: Duration::from_secs(STANDARD_INTERVALL_S),
        }
    }
}

impl Quellen {
    pub fn aus(config: &Config) -> Self {
        Self {
            github: config.github.clone(),
            jira: config.jira.clone(),
            github_oauth: config.github_oauth.clone(),
            atlassian: config.atlassian.clone(),
            endpunkte: Endpoints::default(),
            intervall: Duration::from_secs(config.intervall_s),
        }
    }

    /// Ist irgendetwas eingerichtet, das (mit Schlüssel) laufen könnte?
    fn moeglich(&self) -> bool {
        self.github.as_ref().is_some_and(|g| !g.repos.is_empty()) || self.jira.is_some()
    }
}

/// Warum eine Aktion nicht angewendet wurde.
#[derive(Debug)]
pub enum AktionFehler {
    /// Die Regeln des Workspace lehnen sie ab.
    Regel(ActionError),
    /// Sie lässt sich nicht nach außen abbilden (z. B. kein Repo für Branches).
    Plan(String),
    /// Der Dienst hat den Aufruf abgelehnt oder war nicht erreichbar. Nichts wurde geändert.
    Dienst(String),
    Speichern(String),
    Intern,
}

impl AppState {
    /// Baut die Dienste, die eingerichtet sind **und** einen Schlüssel haben. Blockierend
    /// (.env-Datei).
    pub(crate) fn dienste_bauen(&self) -> Services {
        let inner = &self.inner;
        let quellen = self.quellen();
        let mut dienste = Services::default();
        if let Some(g) = quellen.github.as_ref().filter(|g| !g.repos.is_empty()) {
            // Die Anmeldung per OAuth geht vor dem Token.
            let zugang = match self.oauth_tokens(Service::GitHub) {
                Some(t) => Some(t.access),
                None => self.token(secrets::GITHUB),
            };
            if let Some(t) = zugang {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        dienste.github = Some(GitHubService {
                            client,
                            repos: g.repos.clone(),
                        })
                    }
                    Err(e) => tracing::warn!("GitHub: {e}"),
                }
            }
        }
        if let Some(j) = &quellen.jira
            && let Some(client) = self.jira_client()
        {
            // Ein in der App gewähltes Projekt hat Vorrang vor `projects` und `jql` der Datei.
            let gewaehlt = inner.live.kopie().jira_project;
            let im_projekt = gewaehlt.is_some();
            let jql = match &gewaehlt {
                Some(p) => jql_for_project(p),
                None => jql_for(&j.projects, j.jql.as_deref()),
            };
            match (client, jql) {
                (Ok(client), Ok(jql)) => {
                    dienste.jira = Some(JiraService {
                        client,
                        jql,
                        in_project: im_projekt,
                    })
                }
                (Err(e), _) | (_, Err(e)) => tracing::warn!("Jira: {e}"),
            }
        }
        dienste
    }

    /// Der Jira-Client: per OAuth (über das Atlassian-Gateway), sonst mit E-Mail und API-Token.
    /// `None`, wenn Jira nicht eingerichtet ist oder der Zugang fehlt. Blockierend.
    pub(crate) fn jira_client(&self) -> Option<Result<JiraClient, cockpit_integrations::Error>> {
        if let Some(c) = self.jira_oauth_client() {
            return Some(c);
        }
        let quellen = self.quellen();
        let j = quellen.jira.as_ref().filter(|j| !j.site.is_empty())?;
        let t = self.token(secrets::JIRA)?;
        Some(JiraClient::new(&j.site, &j.email, t))
    }

    /// Der Zugang zu GitHub: die Anmeldung per OAuth geht vor dem Token. Blockierend.
    pub(crate) fn github_zugang(&self) -> Option<cockpit_integrations::Secret> {
        match self.oauth_tokens(Service::GitHub) {
            Some(t) => Some(t.access),
            None => self.token(secrets::GITHUB),
        }
    }

    /// Ein Schlüssel aus der .env-Datei, leer zählt als fehlend. Blockierend.
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
    pub async fn jira_projekte(&self) -> Result<JiraProjectsResponse, AktionFehler> {
        if self.quellen().jira.is_none() {
            return Ok(JiraProjectsResponse::default());
        }
        self.oauth_auffrischen().await;
        let s = self.clone();
        let client = tokio::task::spawn_blocking(move || s.jira_client())
            .await
            .map_err(|_| AktionFehler::Intern)?;
        let Some(client) = client else {
            return Ok(JiraProjectsResponse::default());
        };
        let client = client.map_err(|e| AktionFehler::Plan(format!("Jira: {e}")))?;
        let projekte = client.projects().await.map_err(|e| {
            tracing::warn!("Jira-Projekte: {e}");
            AktionFehler::Dienst(format!("Jira liefert keine Projekte: {e}"))
        })?;
        Ok(JiraProjectsResponse {
            active: true,
            projects: projekte
                .into_iter()
                .map(|p| JiraProject {
                    key: p.key,
                    name: p.name,
                })
                .collect(),
        })
    }

    async fn dienste(&self) -> Services {
        if !self.quellen().moeglich() {
            return Services::default();
        }
        // Zuerst auffrischen (Netzwerk), dann bauen (.env-Datei).
        self.oauth_auffrischen().await;
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.dienste_bauen())
            .await
            .unwrap_or_default()
    }

    async fn lokal_anwenden(
        &self,
        action: Action,
        actor: Actor,
    ) -> Result<ActionResponse, LiveFehler> {
        let s = self.clone();
        tokio::task::spawn_blocking(move || {
            s.inner
                .live
                .anwenden(&s.inner.db, action, actor, jetzt_ms())
        })
        .await
        .unwrap_or_else(|_| Err(LiveFehler::Speichern("Interner Fehler".into())))
    }

    /// Wendet eine Aktion der Oberfläche an (als „Du“). Wirkt sie nach außen, geht erst der
    /// Aufruf an den Dienst – **erst wenn er bestätigt, wird sie lokal angewendet**. Scheitert
    /// er, ändert sich nichts.
    pub async fn aktion_ausfuehren(&self, action: Action) -> Result<ActionResponse, AktionFehler> {
        let inner = &self.inner;
        // Der echte Agent startet nur, wenn alles eingerichtet ist; sonst sagt die Meldung, was fehlt.
        if self.agent_echt() {
            let s = self.clone();
            let a = action.clone();
            let sperre = tokio::task::spawn_blocking(move || s.agent_sperre(&a))
                .await
                .map_err(|_| AktionFehler::Intern)?;
            if let Some(m) = sperre {
                return Err(AktionFehler::Plan(m));
            }
        }
        let _sperre = inner.sperre.lock().await;

        // Probe auf einer Kopie: Die Regeln (auch „nur Du darfst mergen“) gelten vor jedem Aufruf.
        let vorher = inner.live.kopie();
        let mut nachher = vorher.clone();
        nachher
            .apply(action.clone(), Actor::User, jetzt_ms())
            .map_err(AktionFehler::Regel)?;

        let mut ausserhalb = false;
        let dienste = self.dienste().await;
        if !dienste.is_empty() {
            match plane(&vorher, &nachher, &action, &dienste.targets()) {
                Err(e) => return Err(AktionFehler::Plan(e.0)),
                Ok(None) => {}
                Ok(Some(aufruf)) => {
                    dienste.execute(&aufruf).await.map_err(|e| {
                        tracing::warn!("{}: Aufruf gescheitert: {e}", aufruf.service());
                        AktionFehler::Dienst(format!(
                            "{} hat die Aktion nicht ausgeführt: {e}. Es wurde nichts geändert.",
                            aufruf.service()
                        ))
                    })?;
                    inner
                        .geschrieben
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    ausserhalb = true;
                }
            }
        }
        match self.lokal_anwenden(action, Actor::User).await {
            Ok(antwort) => Ok(antwort),
            Err(LiveFehler::Aktion(e)) if ausserhalb => Err(AktionFehler::Dienst(format!(
                "Beim Dienst ausgeführt, lokal aber nicht übernommen ({e}). Der nächste Abgleich holt es nach."
            ))),
            Err(LiveFehler::Aktion(e)) => Err(AktionFehler::Regel(e)),
            Err(LiveFehler::Speichern(m)) => Err(AktionFehler::Speichern(m)),
        }
    }

    /// Ein Abgleich: abrufen und als Aktion `SyncRemote` (Urheber System) anwenden. Quellen im
    /// Kontingent-Limit pausieren. Geschrieben wurde währenddessen? Dann ist der Abruf
    /// veraltet und wird wiederholt.
    pub async fn abgleichen(&self) -> Result<SyncResponse, AktionFehler> {
        let inner = &self.inner;
        let _nur_einer = inner.abgleich.lock().await;
        let dienste = self.dienste().await;
        if dienste.is_empty() {
            return Ok(SyncResponse {
                active: false,
                revision: inner.live.revision(),
                logged: 0,
            });
        }
        for versuch in 0..=WIEDERHOLUNGEN {
            let stand = inner.geschrieben.load(std::sync::atomic::Ordering::SeqCst);
            let jetzt = jetzt_ms();
            let bekannt = inner.live.kopie();
            let pausiert: Vec<Source> = [Source::GitHub, Source::Jira]
                .into_iter()
                .filter(|s| bekannt.sync.paused_until(*s, jetzt).is_some())
                .collect();
            let remote = dienste.fetch(&bekannt, jetzt, &pausiert).await;
            if remote.sources.is_empty() {
                return Ok(SyncResponse {
                    active: true,
                    revision: inner.live.revision(),
                    logged: 0,
                });
            }
            let _sperre = inner.sperre.lock().await;
            let veraltet = inner.geschrieben.load(std::sync::atomic::Ordering::SeqCst) != stand;
            if veraltet && versuch < WIEDERHOLUNGEN {
                continue;
            }
            // Beim letzten Versuch übernehmen wir den Abruf trotzdem; der nächste Takt gleicht aus.
            return match self
                .lokal_anwenden(Action::SyncRemote { remote }, Actor::System)
                .await
            {
                Ok(a) => Ok(SyncResponse {
                    active: true,
                    revision: a.revision,
                    logged: a.logged,
                }),
                Err(LiveFehler::Aktion(e)) => Err(AktionFehler::Regel(e)),
                Err(LiveFehler::Speichern(m)) => Err(AktionFehler::Speichern(m)),
            };
        }
        Err(AktionFehler::Intern)
    }
}

/// Startet den Abgleich im Takt der Konfiguration (der erste sofort).
pub fn abgleich_starten(state: AppState) {
    let intervall = state.quellen().intervall;
    tokio::spawn(async move {
        let mut takt = tokio::time::interval(intervall);
        takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            takt.tick().await;
            match state.abgleichen().await {
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
