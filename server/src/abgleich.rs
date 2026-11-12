//! Jira und GitHub im Server: Dienste aus Konfiguration und Schlüsselbund bauen, Aktionen
//! erst beim Dienst ausführen und dann lokal anwenden, regelmäßig abgleichen.
//!
//! Der Kern der Regeln steht in `cockpit-core` (`sync::merge`) und `cockpit-integrations`
//! (`ausgehend`); hier läuft nur der Ablauf. Ein Schlüssel wird für jeden Gebrauch frisch aus
//! dem Schlüsselbund gelesen (so wirkt `token setzen` ohne Neustart), nie gespeichert oder geloggt.

use std::time::Duration;

use cockpit_core::api::{ActionResponse, JiraProject, JiraProjectsResponse, SyncResponse};
use cockpit_core::{Action, ActionError, Actor, Source};
use cockpit_integrations::ausgehend::plane;
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::{JiraClient, jql_fuer, jql_fuer_projekt};
use cockpit_integrations::oauth::{Dienst, Endpunkte};
use cockpit_integrations::{Dienste, GitHubDienst, JiraDienst};

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
    /// Anmeldung per OAuth (nur Client-IDs; die Secrets liegen im Schlüsselbund).
    pub github_oauth: Option<OAuthKonfig>,
    pub atlassian: Option<AtlassianKonfig>,
    /// Adressen der OAuth-Dienste (Tests setzen nachgebaute ein).
    pub endpunkte: Endpunkte,
    pub intervall: Duration,
}

impl Default for Quellen {
    fn default() -> Self {
        Self {
            github: None,
            jira: None,
            github_oauth: None,
            atlassian: None,
            endpunkte: Endpunkte::default(),
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
            endpunkte: Endpunkte::default(),
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
    /// (Schlüsselbund).
    pub(crate) fn dienste_bauen(&self) -> Dienste {
        let inner = &self.inner;
        let quellen = self.quellen();
        let mut dienste = Dienste::default();
        if let Some(g) = quellen.github.as_ref().filter(|g| !g.repos.is_empty()) {
            // Die Anmeldung per OAuth geht vor dem Token.
            let zugang = match self.oauth_tokens(Dienst::GitHub) {
                Some(t) => Some(t.access),
                None => self.token(secrets::GITHUB),
            };
            if let Some(t) = zugang {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        dienste.github = Some(GitHubDienst {
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
                Some(p) => jql_fuer_projekt(p),
                None => jql_fuer(&j.projects, j.jql.as_deref()),
            };
            match (client, jql) {
                (Ok(client), Ok(jql)) => {
                    dienste.jira = Some(JiraDienst {
                        client,
                        jql,
                        im_projekt,
                    })
                }
                (Err(e), _) | (_, Err(e)) => tracing::warn!("Jira: {e}"),
            }
        }
        dienste
    }

    /// Der Jira-Client: per OAuth (über das Atlassian-Gateway), sonst mit E-Mail und API-Token.
    /// `None`, wenn Jira nicht eingerichtet ist oder der Zugang fehlt. Blockierend.
    pub(crate) fn jira_client(&self) -> Option<Result<JiraClient, cockpit_integrations::Fehler>> {
        if let Some(c) = self.jira_oauth_client() {
            return Some(c);
        }
        let quellen = self.quellen();
        let j = quellen.jira.as_ref().filter(|j| !j.site.is_empty())?;
        let t = self.token(secrets::JIRA)?;
        Some(JiraClient::new(&j.site, &j.email, t))
    }

    /// Ein Schlüssel aus dem Schlüsselbund, leer zählt als fehlend. Blockierend.
    fn token(&self, name: &str) -> Option<cockpit_integrations::Secret> {
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
        let projekte = client.projekte().await.map_err(|e| {
            tracing::warn!("Jira-Projekte: {e}");
            AktionFehler::Dienst(format!("Jira liefert keine Projekte: {e}"))
        })?;
        Ok(JiraProjectsResponse {
            aktiv: true,
            projekte: projekte
                .into_iter()
                .map(|p| JiraProject {
                    key: p.key,
                    name: p.name,
                })
                .collect(),
        })
    }

    async fn dienste(&self) -> Dienste {
        if !self.quellen().moeglich() {
            return Dienste::default();
        }
        // Zuerst auffrischen (Netzwerk), dann bauen (Schlüsselbund).
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
        let _sperre = inner.sperre.lock().await;

        // Probe auf einer Kopie: Die Regeln (auch „nur Du darfst mergen“) gelten vor jedem Aufruf.
        let vorher = inner.live.kopie();
        let mut nachher = vorher.clone();
        nachher
            .apply(action.clone(), Actor::Du, jetzt_ms())
            .map_err(AktionFehler::Regel)?;

        let mut ausserhalb = false;
        let dienste = self.dienste().await;
        if !dienste.ist_leer() {
            match plane(&vorher, &nachher, &action, &dienste.ziele()) {
                Err(e) => return Err(AktionFehler::Plan(e.0)),
                Ok(None) => {}
                Ok(Some(aufruf)) => {
                    dienste.ausfuehren(&aufruf).await.map_err(|e| {
                        tracing::warn!("{}: Aufruf gescheitert: {e}", aufruf.dienst());
                        AktionFehler::Dienst(format!(
                            "{} hat die Aktion nicht ausgeführt: {e}. Es wurde nichts geändert.",
                            aufruf.dienst()
                        ))
                    })?;
                    inner
                        .geschrieben
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    ausserhalb = true;
                }
            }
        }
        match self.lokal_anwenden(action, Actor::Du).await {
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
        if dienste.ist_leer() {
            return Ok(SyncResponse {
                aktiv: false,
                revision: inner.live.revision(),
                protokolliert: 0,
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
            let remote = dienste.holen(&bekannt, jetzt, &pausiert).await;
            if remote.sources.is_empty() {
                return Ok(SyncResponse {
                    aktiv: true,
                    revision: inner.live.revision(),
                    protokolliert: 0,
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
                    aktiv: true,
                    revision: a.revision,
                    protokolliert: a.protokolliert,
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
                Ok(a) if a.protokolliert > 0 => {
                    tracing::info!("Abgleich: {} neue Einträge im Protokoll", a.protokolliert)
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Abgleich nicht übernommen: {e:?}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use cockpit_core::{Workspace, persist};
    use cockpit_integrations::Secret;
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::app::router;
    use crate::db;
    use crate::secrets::{MemoryStore, SecretStore};

    const TOKEN: &str = "ghp_streng_geheim_42";

    fn quellen(api: &str) -> Quellen {
        Quellen {
            github: Some(GitHubKonfig {
                api_url: api.into(),
                repos: vec!["a/x".into()],
            }),
            jira: None,
            intervall: Duration::from_secs(120),
            ..Quellen::default()
        }
    }

    fn zustand(quellen: Quellen, mit_token: bool) -> AppState {
        let secrets = Arc::new(MemoryStore::new());
        if mit_token {
            secrets.set(secrets::GITHUB, &Secret::new(TOKEN)).unwrap();
        }
        let conn = db::open_im_speicher().unwrap();
        AppState::mit_quellen(conn, secrets, 8787, quellen)
    }

    fn anfrage(methode: &str, pfad: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(methode)
            .uri(pfad)
            .header("host", "127.0.0.1:8787")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
    }

    async fn sende(state: &AppState, req: Request<Body>) -> (StatusCode, String) {
        let app: Router = router(state.clone(), PathBuf::from("/x"));
        let antwort = app.oneshot(req).await.unwrap();
        let status = antwort.status();
        let body = antwort.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn sync(state: &AppState) -> (StatusCode, serde_json::Value) {
        let req = anfrage("POST", "/api/sync")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();
        let (s, b) = sende(state, req).await;
        (s, serde_json::from_str(&b).unwrap_or(json!({"roh": b})))
    }

    async fn aktion(state: &AppState, action: serde_json::Value) -> (StatusCode, String) {
        let req = anfrage("POST", "/api/actions")
            .header("content-type", "application/json")
            .body(Body::from(json!({"action": action}).to_string()))
            .unwrap();
        sende(state, req).await
    }

    async fn stand(state: &AppState) -> (Workspace, u64) {
        let req = anfrage("GET", "/api/workspace")
            .body(Body::empty())
            .unwrap();
        let (_, body) = sende(state, req).await;
        persist::load_with_revision(&body).unwrap()
    }

    async fn health(state: &AppState) -> serde_json::Value {
        let req = anfrage("GET", "/api/health").body(Body::empty()).unwrap();
        serde_json::from_str(&sende(state, req).await.1).unwrap()
    }

    fn ok(v: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(v)
    }

    async fn get(s: &MockServer, p: &str, a: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(a)
            .mount(s)
            .await;
    }

    /// GitHub mit einem Pull Request (#7, Review angefragt, Checks grün).
    async fn github() -> MockServer {
        let s = MockServer::start().await;
        get(&s, "/user", ok(json!({"login": "ich"}))).await;
        for (q, treffer) in [
            (
                "is:pr is:open archived:false review-requested:@me repo:a/x",
                true,
            ),
            ("is:pr is:open archived:false author:@me repo:a/x", false),
        ] {
            let items = if treffer {
                json!([{"number": 7, "repository_url": "https://api.github.com/repos/a/x"}])
            } else {
                json!([])
            };
            Mock::given(method("GET"))
                .and(path("/search/issues"))
                .and(query_param("q", q))
                .respond_with(ok(json!({"items": items})))
                .mount(&s)
                .await;
        }
        get(
            &s,
            "/repos/a/x/pulls/7",
            ok(json!({
            "number": 7, "title": "Avatar", "user": {"login": "lena"},
            "head": {"ref": "feature/x", "sha": "s1"}, "base": {"ref": "main"},
            "updated_at": "2024-05-14T10:00:00Z"})),
        )
        .await;
        get(&s, "/repos/a/x/pulls/7/files", ok(json!([
            {"filename": "a.rs", "patch": "@@ -1,2 +1,2 @@\n fn a() {\n-    alt();\n+    neu();\n }"}]))).await;
        get(&s, "/repos/a/x/issues/7/comments", ok(json!([]))).await;
        get(&s, "/repos/a/x/pulls/7/comments", ok(json!([]))).await;
        get(&s, "/repos/a/x/pulls/7/reviews", ok(json!([]))).await;
        get(
            &s,
            "/repos/a/x/commits/s1/check-runs",
            ok(json!({"check_runs": [
            {"id": 5, "name": "Tests", "status": "completed", "conclusion": "success"}]})),
        )
        .await;
        s
    }

    fn jira_zustand(site: &str) -> AppState {
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .set(secrets::JIRA, &Secret::new("jira-geheim"))
            .unwrap();
        let quellen = Quellen {
            jira: Some(JiraKonfig {
                site: site.into(),
                email: "ich@firma.example".into(),
                projects: vec!["ALT".into()],
                jql: None,
            }),
            ..Quellen::default()
        };
        AppState::mit_quellen(db::open_im_speicher().unwrap(), secrets, 8787, quellen)
    }

    #[tokio::test]
    async fn jira_projekte_fuer_die_auswahl() {
        // Ohne Jira: nichts zu wählen, aber kein Fehler.
        let state = zustand(Quellen::default(), false);
        let req = anfrage("GET", "/api/jira/projekte")
            .body(Body::empty())
            .unwrap();
        let (status, body) = sende(&state, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["aktiv"],
            false
        );

        let j = MockServer::start().await;
        get(
            &j,
            "/rest/api/3/project/search",
            ok(json!({"isLast": true, "values": [
                {"key": "BAU", "name": "Baukasten"}, {"key": "WEB", "name": "Webseite"}]})),
        )
        .await;
        let state = jira_zustand(&j.uri());
        let req = anfrage("GET", "/api/jira/projekte")
            .body(Body::empty())
            .unwrap();
        let (status, body) = sende(&state, req).await;
        assert_eq!(status, StatusCode::OK);
        let antwort: JiraProjectsResponse = serde_json::from_str(&body).unwrap();
        assert!(antwort.aktiv);
        assert_eq!(
            antwort.projekte,
            [
                JiraProject {
                    key: "BAU".into(),
                    name: "Baukasten".into()
                },
                JiraProject {
                    key: "WEB".into(),
                    name: "Webseite".into()
                },
            ]
        );
        assert!(!body.contains("jira-geheim"));
    }

    #[tokio::test]
    async fn gewaehltes_projekt_bestimmt_die_abfrage() {
        let j = MockServer::start().await;
        get(&j, "/rest/api/3/myself", ok(json!({"accountId": "ich"}))).await;
        let issue = json!({"key": "WEB-7", "fields": {"summary": "Fremdes Ticket",
            "status": {"name": "To Do", "statusCategory": {"key": "new"}},
            "assignee": {"accountId": "lena", "displayName": "Lena Koch"},
            "updated": "2024-05-14T10:00:00Z"}});
        Mock::given(method("GET"))
            .and(path("/rest/api/3/search/jql"))
            .and(query_param(
                "jql",
                "project = WEB AND statusCategory != Done ORDER BY updated DESC",
            ))
            .respond_with(ok(json!({"issues": [issue]})))
            .expect(1)
            .mount(&j)
            .await;
        get(
            &j,
            "/rest/api/3/issue/WEB-7/comment",
            ok(json!({"comments": []})),
        )
        .await;

        let state = jira_zustand(&j.uri());
        let (status, _) = aktion(&state, json!({"SelectJiraProject": {"project": "WEB"}})).await;
        assert_eq!(status, StatusCode::OK);
        let (status, antwort) = sync(&state).await;
        assert_eq!((status, &antwort["aktiv"]), (StatusCode::OK, &json!(true)));
        let (ws, _) = stand(&state).await;
        let item = ws
            .item_by_key("WEB-7")
            .expect("Ticket aus dem gewählten Projekt");
        assert_eq!(item.reason_text, "Im Projekt");
        assert_eq!(ws.jira_project.as_deref(), Some("WEB"));
    }

    #[tokio::test]
    async fn sync_ohne_quelle_laesst_die_demo_daten() {
        let state = zustand(Quellen::default(), false);
        let (status, antwort) = sync(&state).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(antwort["aktiv"], false);
        let (ws, revision) = stand(&state).await;
        assert_eq!(ws, cockpit_core::demo::workspace());
        assert_eq!(revision, 0);
        let h = health(&state).await;
        assert_eq!(h["abgleich"]["github"], "aus");
        assert_eq!(h["abgleich"]["jira"], "aus");
    }

    #[tokio::test]
    async fn quelle_ohne_schluessel_bleibt_aus() {
        let g = github().await;
        let state = zustand(quellen(&g.uri()), false);
        let (_, antwort) = sync(&state).await;
        assert_eq!(antwort["aktiv"], false);
        assert!(g.received_requests().await.unwrap().is_empty());
        assert_eq!(health(&state).await["abgleich"]["github"], "aus");
    }

    #[tokio::test]
    async fn post_sync_holt_pull_requests_und_zeigt_den_zustand() {
        let g = github().await;
        let state = zustand(quellen(&g.uri()), true);
        let (status, antwort) = sync(&state).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(antwort["aktiv"], true);
        assert_eq!(antwort["revision"], 1);

        let (ws, _) = stand(&state).await;
        assert!(ws.item("gh-a-x-7").is_some());
        assert!(ws.item("bau142").is_none(), "Beispieldaten sind weg");
        assert_eq!(ws.sync.pr(7).unwrap().repo, "a/x");
        assert_eq!(ws.review(7).unwrap().files[0].name, "a.rs");
        let h = health(&state).await;
        assert_eq!(h["abgleich"]["github"], "ok");
        assert_eq!(h["abgleich"]["jira"], "aus");
        assert_eq!(h["abgleich"]["intervall_s"], 120);
        // Jeder Abruf trug den Schlüssel – und nur dort.
        let anfragen = g.received_requests().await.unwrap();
        assert!(
            anfragen
                .iter()
                .all(|r| r.headers.get("authorization").is_some())
        );
        assert!(!persist::save(&ws).contains(TOKEN));
    }

    #[tokio::test]
    async fn abgelaufenes_token_wird_zum_zustand_und_nichts_geht_verloren() {
        let g = github().await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        g.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&g)
            .await;
        let (status, _) = sync(&state).await;
        assert_eq!(status, StatusCode::OK);
        let (ws, _) = stand(&state).await;
        assert!(ws.item("gh-a-x-7").is_some());
        assert_eq!(
            health(&state).await["abgleich"]["github"],
            "nicht-angemeldet"
        );
    }

    #[tokio::test]
    async fn kontingent_pausiert_den_abgleich() {
        let g = github().await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        g.reset().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("x-ratelimit-remaining", "0")
                    .insert_header("retry-after", "600"),
            )
            .mount(&g)
            .await;
        sync(&state).await;
        assert_eq!(health(&state).await["abgleich"]["github"], "kontingent");
        let vorher = g.received_requests().await.unwrap().len();
        // Während der Pause fragt der nächste Abgleich gar nicht erst.
        sync(&state).await;
        assert_eq!(g.received_requests().await.unwrap().len(), vorher);
    }

    #[tokio::test]
    async fn antwort_geht_erst_an_github_dann_wird_sie_lokal_angewendet() {
        let g = github().await;
        Mock::given(method("POST"))
            .and(path("/repos/a/x/issues/7/comments"))
            .and(wiremock::matchers::header(
                "authorization",
                format!("Bearer {TOKEN}").as_str(),
            ))
            .and(wiremock::matchers::body_json(json!({"body": "Danke!"})))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
            .expect(1)
            .mount(&g)
            .await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        let (status, body) = aktion(
            &state,
            json!({"Reply": {"item": "gh-a-x-7", "text": "Danke!"}}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (ws, _) = stand(&state).await;
        let c = ws
            .item("gh-a-x-7")
            .unwrap()
            .comments
            .last()
            .unwrap()
            .clone();
        assert!(c.own && c.text == "Danke!");
        assert_eq!(ws.log.last().unwrap().actor, Actor::Du);
    }

    #[tokio::test]
    async fn schlaegt_der_aufruf_fehl_aendert_sich_nichts() {
        let g = github().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(
                json!({"message": format!("Resource not accessible by token {TOKEN}")}),
            ))
            .mount(&g)
            .await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        let (vorher, rev) = stand(&state).await;
        let (status, body) =
            aktion(&state, json!({"Reply": {"item": "gh-a-x-7", "text": "Hi"}})).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            body.contains("GitHub hat die Aktion nicht ausgeführt"),
            "{body}"
        );
        assert!(body.contains("Es wurde nichts geändert"));
        assert!(
            !body.contains(TOKEN),
            "der Schlüssel taucht nie auf: {body}"
        );
        let (nachher, rev2) = stand(&state).await;
        assert_eq!((vorher, rev), (nachher, rev2));

        // Ablehnung (422) bekommt die Meldung des Dienstes.
        g.reset().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(422).set_body_json(json!({"message": "Validation Failed"})),
            )
            .mount(&g)
            .await;
        let (status, body) =
            aktion(&state, json!({"Reply": {"item": "gh-a-x-7", "text": "Hi"}})).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(body.contains("Validation Failed"), "{body}");
    }

    #[tokio::test]
    async fn regeln_gelten_vor_dem_aufruf() {
        let g = github().await;
        // Mergen ohne gesendetes Review: die Regeln lehnen ab, GitHub hört nichts davon.
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&g)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&g)
            .await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        let (status, body) = aktion(
            &state,
            json!({"MergePr": {"review": 7, "method": "Squash"}}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body.contains("Mergen geht noch nicht"));
        // Der Abgleich ist Sache des Systems, nicht der Oberfläche.
        let (status, _) = aktion(&state, json!({"SyncRemote": {"remote": {}}})).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn review_und_merge_gehen_nach_github() {
        let g = github().await;
        Mock::given(method("POST"))
            .and(path("/repos/a/x/pulls/7/reviews"))
            .and(wiremock::matchers::body_json(json!({
                "event": "APPROVE", "commit_id": "s1",
                "comments": [{"path": "a.rs", "line": 2, "side": "RIGHT",
                              "body": "Schöner so\n\n```suggestion\n    neuer();\n```"}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
            .expect(1)
            .mount(&g)
            .await;
        Mock::given(method("PUT"))
            .and(path("/repos/a/x/pulls/7/merge"))
            .and(wiremock::matchers::body_json(
                json!({"merge_method": "squash", "sha": "s1"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"merged": true})))
            .expect(1)
            .mount(&g)
            .await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        // Diff: Kontext, entfernt, neu, Kontext → Index 2 ist die neue Zeile (Nr. 2).
        let (s, b) = aktion(&state, json!({"AddLineComment": {
            "review": 7, "file": "a.rs", "line": 2, "text": "Schöner so", "suggestion": "    neuer();"}})).await;
        assert_eq!(s, StatusCode::OK, "{b}");
        aktion(
            &state,
            json!({"SetVerdict": {"review": 7, "verdict": "Approve"}}),
        )
        .await;
        let (s, b) = aktion(&state, json!({"SendReview": {"review": 7, "comment": ""}})).await;
        assert_eq!(s, StatusCode::OK, "{b}");
        let (s, b) = aktion(
            &state,
            json!({"MergePr": {"review": 7, "method": "Squash"}}),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{b}");
        let (ws, _) = stand(&state).await;
        assert!(ws.review(7).unwrap().sent && ws.review(7).unwrap().merged.is_some());
    }

    #[tokio::test]
    async fn andere_aktionen_bleiben_lokal() {
        let g = github().await;
        let state = zustand(quellen(&g.uri()), true);
        sync(&state).await;
        let vorher = g.received_requests().await.unwrap().len();
        let (s, _) = aktion(&state, json!({"MarkDone": {"item": "gh-a-x-7"}})).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(g.received_requests().await.unwrap().len(), vorher);
    }
}
