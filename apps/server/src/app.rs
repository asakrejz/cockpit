//! Der HTTP-Teil: Routen unter `/api`, dahinter die gebaute Web-App.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use cockpit_core::api::{ActionRequest, ErrorBody, ServerEvent};
use rusqlite::Connection;
use serde::Serialize;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeDir;

use crate::abgleich::{AktionFehler, Quellen};
use crate::config::{AgentWahl, Config, Seed};
use crate::container::Zwischenspeicher;
use crate::db;
use crate::live::Live;
use crate::secrets::{self, SecretStore};
use crate::zugang;
use cockpit_core::api::{AgentStatus, ContainerStatus, EndpointStatus};
use cockpit_core::{AuthKind, ServiceStatus};
use cockpit_integrations::oauth::Service;

/// So oft macht der simulierte Agent einen Schritt (wie im Demo-Modus der App).
pub const AGENT_TAKT: Duration = Duration::from_millis(2200);
/// So oft kommt ein Lebenszeichen auf der Ereignis-Verbindung.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Einstellungen aus `config.toml` und Umgebung, die nicht Jira oder GitHub betreffen.
#[derive(Debug, Clone, Default)]
pub struct Einstellungen {
    pub seed: Seed,
    /// `[agent] anbieter` und `[agent.endpunkt]`.
    pub agent: AgentWahl,
    /// Ersatz für `https://api.anthropic.com`, damit Tests nie die echte API erreichen.
    pub anthropic_api: Option<String>,
    /// `[agent] container`.
    pub container: Option<String>,
    /// `COCKPIT_CONTAINER` (geht der Datei vor).
    pub container_env: Option<String>,
    /// Wo nach `docker` und `podman` gesucht wird; `None` = `PATH` des Prozesses (Tests setzen es).
    pub suchpfad: Option<std::ffi::OsString>,
    /// `COCKPIT_DEV_PORT`: der Port von `trunk serve`, dessen Anfragen (Host, Origin) auch gelten.
    pub dev_port: Option<u16>,
    /// Die `config.toml`, in die der Einrichtungsassistent schreibt.
    pub konfig_pfad: Option<PathBuf>,
    /// `[agent] repo`: das Repo, in dem der Agent arbeitet.
    pub agent_repo: Option<String>,
    /// Ordner mit Containerfile und Läufer des Agenten.
    pub agent_dir: Option<PathBuf>,
    /// Basisadresse zum Klonen statt GitHub (nur Tests).
    pub git_basis: Option<String>,
    /// Hier legt der Agent Klone und Aufträge ab (neben der Datenbank).
    pub daten_dir: Option<PathBuf>,
    /// Feste Obergrenze in Sekunden für einen Lauf statt der Grenzen des Workspace (nur Tests).
    pub lauf_sekunden: Option<u64>,
}

impl Einstellungen {
    pub fn aus(config: &Config) -> Self {
        Self {
            seed: config.seed,
            agent: AgentWahl {
                anbieter: config.anbieter,
                endpunkt: config.endpunkt.clone(),
            },
            anthropic_api: None,
            container: config.container.clone(),
            container_env: config.container_env.clone(),
            suchpfad: None,
            dev_port: config.dev_port,
            konfig_pfad: config.konfig_pfad.clone(),
            agent_repo: config.agent_repo.clone(),
            agent_dir: Some(agent_ordner_finden(config)),
            git_basis: config.git_basis.clone(),
            daten_dir: config.db.parent().map(PathBuf::from),
            lauf_sekunden: None,
        }
    }
}

/// Der Ordner `agent` mit Containerfile und Läufer: `[agent] dir` bzw. `COCKPIT_AGENT_DIR`, sonst
/// neben der gebauten App (`<dist>/../../agent`, so liegt er im Repo und im Release-Archiv),
/// sonst `./apps/agent`.
fn agent_ordner_finden(config: &Config) -> PathBuf {
    if let Some(d) = &config.agent_dir {
        return d.clone();
    }
    let neben_dist = config.dist.join("..").join("..").join("agent");
    if neben_dist.join("Containerfile").is_file() {
        return neben_dist;
    }
    PathBuf::from("apps/agent")
}

/// Gemeinsamer Zustand der Handler. Klonen ist billig.
#[derive(Clone)]
pub struct AppState {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) db: Mutex<Connection>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) live: Live,
    /// Der Port, auf dem der Server lauscht – für die Prüfung von `Host` und `Origin`.
    pub(crate) port: u16,
    /// Offene OAuth-Anmeldungen (`state` → PKCE-Verifier), nur im Speicher.
    pub(crate) anmeldungen: Mutex<cockpit_integrations::oauth::SignIns>,
    /// Nur eine Auffrischung der OAuth-Tokens gleichzeitig (Refresh-Tokens drehen sich).
    pub(crate) auffrischen: tokio::sync::Mutex<()>,
    /// Jira und GitHub, wie konfiguriert (ob sie laufen, hängt noch vom Schlüssel ab).
    /// Wird beim Speichern im Einrichtungsassistenten ausgetauscht (ohne Neustart).
    pub(crate) quellen: RwLock<Arc<Quellen>>,
    /// Anbieter und Endpunkt des Agenten, ebenfalls austauschbar.
    pub(crate) agent_wahl: RwLock<AgentWahl>,
    /// Lesen, Ändern und Schreiben von `config.toml` gehen nacheinander.
    pub(crate) konfig_sperre: Mutex<()>,
    /// Schreibzugriffe nach außen und das Übernehmen eines Abgleichs gehen nacheinander.
    pub(crate) sperre: tokio::sync::Mutex<()>,
    /// Zählt erfolgreiche Schreibzugriffe: Ein Abruf, der währenddessen begann, ist veraltet.
    pub(crate) geschrieben: AtomicU64,
    /// Nur ein Abgleich gleichzeitig.
    pub(crate) abgleich: tokio::sync::Mutex<()>,
    pub(crate) einstellungen: Einstellungen,
    /// Ergebnis der Suche nach der Container-Laufzeit, kurz gemerkt.
    pub(crate) container: Zwischenspeicher,
    /// Läufe des echten Agenten (Container) und der Bau des Images.
    pub(crate) agent: crate::agent_betrieb::Laeufe,
}

impl AppState {
    /// Wie [`AppState::mit_port`], mit dem Standard-Port (für Tests).
    #[cfg(test)]
    pub fn new(db: Connection, secrets: Arc<dyn SecretStore>) -> Self {
        Self::mit_port(db, secrets, crate::config::STANDARD_PORT)
    }

    /// Lädt dabei den gespeicherten Workspace (sonst die Beispieldaten).
    #[cfg(test)]
    pub fn mit_port(db: Connection, secrets: Arc<dyn SecretStore>, port: u16) -> Self {
        Self::mit_quellen(db, secrets, port, Quellen::default())
    }

    /// Wie [`AppState::mit_port`], mit Jira und GitHub für den Abgleich. Die Tests starten mit
    /// den Beispieldaten, der Server selbst mit leerem Workspace ([`AppState::neu`]).
    #[cfg(test)]
    pub fn mit_quellen(
        db: Connection,
        secrets: Arc<dyn SecretStore>,
        port: u16,
        quellen: Quellen,
    ) -> Self {
        let einstellungen = Einstellungen {
            seed: Seed::Demo,
            // Tests suchen nirgends nach Container-Programmen: gleiches Ergebnis auf jedem Rechner.
            suchpfad: Some(std::ffi::OsString::new()),
            ..Einstellungen::default()
        };
        Self::neu(db, secrets, port, quellen, einstellungen)
    }

    /// Der Zustand des Servers. Ein gespeicherter Workspace wird geladen, sonst gilt der Seed.
    pub fn neu(
        db: Connection,
        secrets: Arc<dyn SecretStore>,
        port: u16,
        quellen: Quellen,
        einstellungen: Einstellungen,
    ) -> Self {
        let live = Live::laden(&db, einstellungen.seed);
        Self {
            inner: Arc::new(Inner {
                db: Mutex::new(db),
                secrets,
                live,
                port,
                anmeldungen: crate::oauth::neue_anmeldungen(),
                auffrischen: tokio::sync::Mutex::new(()),
                quellen: RwLock::new(Arc::new(quellen)),
                agent_wahl: RwLock::new(einstellungen.agent.clone()),
                konfig_sperre: Mutex::new(()),
                sperre: tokio::sync::Mutex::new(()),
                geschrieben: AtomicU64::new(0),
                abgleich: tokio::sync::Mutex::new(()),
                einstellungen,
                container: Zwischenspeicher::default(),
                agent: crate::agent_betrieb::Laeufe::default(),
            }),
        }
    }

    /// Jira und GitHub, wie zuletzt konfiguriert.
    pub(crate) fn quellen(&self) -> Arc<Quellen> {
        match self.inner.quellen.read() {
            Ok(q) => q.clone(),
            Err(vergiftet) => vergiftet.into_inner().clone(),
        }
    }

    /// Anbieter und Endpunkt des Agenten, wie zuletzt konfiguriert.
    pub(crate) fn agent_wahl(&self) -> AgentWahl {
        match self.inner.agent_wahl.read() {
            Ok(l) => l.clone(),
            Err(vergiftet) => vergiftet.into_inner().clone(),
        }
    }

    /// Übernimmt eine neu geschriebene Konfiguration ohne Neustart: Quellen, OAuth-Apps und
    /// Agent-Endpunkt. Port, Datenbank, Abgleich-Takt und Container-Wahl bleiben, wie sie sind.
    pub(crate) fn konfig_anwenden(&self, c: &Config) {
        let alt = self.quellen();
        let neu = Quellen {
            github: c.github.clone(),
            jira: c.jira.clone(),
            github_oauth: c.github_oauth.clone(),
            atlassian: c.atlassian.clone(),
            endpunkte: alt.endpunkte.clone(),
            intervall: alt.intervall,
        };
        match self.inner.quellen.write() {
            Ok(mut q) => *q = Arc::new(neu),
            Err(v) => *v.into_inner() = Arc::new(neu),
        }
        let wahl = AgentWahl {
            anbieter: c.anbieter,
            endpunkt: c.endpunkt.clone(),
        };
        match self.inner.agent_wahl.write() {
            Ok(mut l) => *l = wahl,
            Err(v) => *v.into_inner() = wahl,
        }
    }

    /// Ein Takt der Aufgaben im Hintergrund. Der simulierte Agent macht nur mit, wenn der Server
    /// mit dem Seed `demo` läuft; sonst arbeitet der echte Agent in Containern. Blockierend.
    pub fn takt(&self) {
        self.inner
            .live
            .takt(&self.inner.db, jetzt_ms(), !self.agent_echt());
    }
}

pub(crate) fn jetzt_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Startet die Hintergrundarbeit des Agenten im Server: im Modus „echt“ der Aufpasser für die
/// Container, mit dem Seed `demo` der Takt des simulierten Agenten (statt im Browser).
pub fn agent_takt_starten(state: AppState) {
    if state.agent_echt() {
        crate::agent_betrieb::aufpasser_starten(state);
        return;
    }
    tokio::spawn(async move {
        let mut takt = tokio::time::interval(AGENT_TAKT);
        takt.tick().await; // der erste Tick kommt sofort
        loop {
            takt.tick().await;
            let state = state.clone();
            let _ = tokio::task::spawn_blocking(move || state.takt()).await;
        }
    });
}

/// Ob eine Verbindung eingerichtet ist: Sie steht in der Datenbank **und** ihr Schlüssel
/// liegt in der Schlüsselablage. Der Schlüssel selbst wird nie gelesen oder ausgegeben.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Verbindungen {
    pub github: bool,
    pub jira: bool,
}

/// Art und Stand der Anmeldung je Dienst – für „Mit … anmelden“ und „Trennen“.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Anmeldung {
    pub github: ServiceStatus,
    pub jira: ServiceStatus,
}

/// Zustand des Abgleichs je Quelle: `aus` (nicht eingerichtet oder ohne Schlüssel), `wartet`
/// (noch kein Abruf), `ok`, `nicht-angemeldet`, `keine-berechtigung`, `kontingent`,
/// `offline` oder `fehler`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AbgleichZustand {
    pub github: String,
    pub jira: String,
    pub intervall_s: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    pub verbindungen: Verbindungen,
    pub anmeldung: Anmeldung,
    pub abgleich: AbgleichZustand,
    /// Anbieter, Adresse, Modell und ob ein Key liegt – nie der Key.
    pub endpunkt: EndpointStatus,
    pub container: ContainerStatus,
    /// Ob der echte Agent loslaufen kann und was fehlt.
    pub agent: AgentStatus,
    /// Wo die `.env`-Datei mit den Schlüsseln liegt und was daran zu warnen ist – nie ein Wert.
    pub env_datei: Option<secrets::EnvFileStatus>,
}

impl AppState {
    /// Blockierend (Datenbank, .env-Datei) – nicht direkt in einem async-Handler aufrufen.
    fn health(&self) -> Health {
        let namen = match self.inner.db.lock() {
            Ok(conn) => db::verbindungen(&conn).unwrap_or_else(|e| {
                tracing::warn!("Verbindungen konnten nicht gelesen werden: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        // Ein Token (Datenbank-Eintrag **und** Schlüssel) oder die Anmeldung per OAuth.
        let eingerichtet = |name: &str| {
            namen.iter().any(|n| n == name)
                && self.inner.secrets.has(name).unwrap_or_else(|e| {
                    tracing::warn!("Schlüssel „{name}“ nicht prüfbar: {e}");
                    false
                })
        };
        let github = self.dienst_status(Service::GitHub, eingerichtet(secrets::GITHUB));
        // Jira per Token braucht Adresse und E-Mail aus der Konfiguration (Eintrag in der Datenbank).
        let jira = self.dienst_status(Service::Atlassian, eingerichtet(secrets::JIRA));
        let verbindungen = Verbindungen {
            github: github.connected,
            jira: jira.connected,
        };
        let hat_schluessel = |name: &str| self.inner.secrets.has(name).unwrap_or(false);
        let ws = self.inner.live.kopie();
        let zustand = |aktiv: bool, quelle: cockpit_core::Source| {
            if !aktiv {
                return "aus".to_string();
            }
            ws.sync
                .state_of(quelle)
                .map_or("wartet", cockpit_core::SourceState::id)
                .to_string()
        };
        let q = self.quellen();
        let wahl = self.agent_wahl();
        let endpunkt = EndpointStatus {
            configured: wahl.endpunkt.is_some(),
            provider: wahl.anbieter,
            url: wahl.endpunkt.as_ref().and_then(|e| e.url.clone()),
            model: wahl.endpunkt.as_ref().and_then(|e| e.modell.clone()),
            key: hat_schluessel(secrets::AGENT_KEY) || hat_schluessel(secrets::LITELLM),
        };
        let container = self.container_status();
        let agent = self.agent_status();
        Health {
            status: "ok",
            version: VERSION,
            abgleich: AbgleichZustand {
                github: zustand(
                    q.github.as_ref().is_some_and(|g| !g.repos.is_empty())
                        && (github.kind == AuthKind::OAuth || hat_schluessel(secrets::GITHUB)),
                    cockpit_core::Source::GitHub,
                ),
                jira: zustand(
                    q.jira.is_some()
                        && (jira.kind == AuthKind::OAuth || hat_schluessel(secrets::JIRA)),
                    cockpit_core::Source::Jira,
                ),
                intervall_s: q.intervall.as_secs(),
            },
            verbindungen,
            anmeldung: Anmeldung { github, jira },
            endpunkt,
            container,
            agent,
            env_datei: self.inner.secrets.env_file().map(secrets::env_file_status),
        }
    }
}

async fn health(State(state): State<AppState>) -> Result<Json<Health>, StatusCode> {
    tokio::task::spawn_blocking(move || state.health())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Der Key des Endpunkts aus der .env-Datei: `agent-key`, ersatzweise das ältere `litellm`.
/// Blockierend.
pub(crate) fn agent_key_lesen(
    secrets: &dyn crate::secrets::SecretStore,
) -> Result<Option<cockpit_integrations::Secret>, crate::secrets::SecretFehler> {
    match secrets.get(secrets::AGENT_KEY)? {
        Some(k) => Ok(Some(k)),
        None => secrets.get(secrets::LITELLM),
    }
}

impl AppState {
    /// Wohin die Aufrufe an den Endpunkt gehen: die eingetragene Adresse, sonst direkt Anthropic.
    pub(crate) fn endpunkt_ziel(&self) -> cockpit_integrations::endpoint::Target {
        let wahl = self.agent_wahl();
        cockpit_integrations::endpoint::Target::from(
            wahl.endpunkt.as_ref().and_then(|e| e.url.as_deref()),
            self.inner.einstellungen.anthropic_api.as_deref(),
        )
    }

    /// Der Key des Endpunkts. `Err` ist schon eine fertige Antwort.
    async fn endpunkt_key(&self) -> Result<Option<cockpit_integrations::Secret>, Box<Response>> {
        let secrets = self.inner.secrets.clone();
        match tokio::task::spawn_blocking(move || agent_key_lesen(secrets.as_ref())).await {
            Ok(Ok(k)) => Ok(k),
            Ok(Err(e)) => {
                tracing::warn!("Key des Agent-Endpunkts nicht lesbar: {e}");
                Err(Box::new(fehler(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Die .env-Datei im Projektordner ist nicht lesbar",
                )))
            }
            Err(_) => Err(Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response())),
        }
    }
}

/// `GET /api/agent/pruefen` (Alias `/api/litellm/pruefen`): ruft nur `GET <url>/v1/models` mit dem
/// Key auf und meldet, ob der Endpunkt erreichbar ist, den Key annimmt und das Modell anbietet.
/// Nie der Key.
async fn endpunkt_pruefen(State(state): State<AppState>) -> Response {
    let wahl = state.agent_wahl();
    let Some(e) = wahl.endpunkt.clone() else {
        // Ohne Eintrag gibt es nichts zu prüfen (auch nicht „direkt“: erst wählt die Person ein Modell).
        return Json(cockpit_core::api::EndpointCheck {
            hint: Some(
                "Der Endpunkt ist nicht eingerichtet. Wähle im Schritt „Modell“ Anbieter, Endpunkt und Modell."
                    .into(),
            ),
            ..Default::default()
        })
        .into_response();
    };
    let key = match state.endpunkt_key().await {
        Ok(k) => k,
        Err(antwort) => return *antwort,
    };
    Json(
        cockpit_integrations::endpoint::verify(
            &state.endpunkt_ziel(),
            key.as_ref(),
            e.modell.as_deref(),
        )
        .await,
    )
    .into_response()
}

/// `GET /api/agent/modelle`: die Modell-IDs des Endpunkts (nur IDs, sortiert) für das Drop-down.
/// Der Key kommt aus der .env-Datei und verlässt das Backend nie.
async fn agent_modelle(State(state): State<AppState>) -> Response {
    let key = match state.endpunkt_key().await {
        Ok(k) => k,
        Err(antwort) => return *antwort,
    };
    Json(cockpit_integrations::endpoint::models_load(&state.endpunkt_ziel(), key.as_ref()).await)
        .into_response()
}

/// `GET /api/agent/image`: Stand von „Image bauen“ (läuft, vorhanden, letzte Zeilen, Fehler).
async fn agent_image_lesen(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_stand()).await {
        Ok(b) => Json(b).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/agent/image`: baut das Agent-Image im Hintergrund (`docker build` bzw. `podman build`).
/// Der Fortschritt steht in `GET /api/agent/image`.
async fn agent_image_bauen(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_bauen()).await {
        Ok(Ok(b)) => Json(b).into_response(),
        Ok(Err(m)) => fehler(StatusCode::UNPROCESSABLE_ENTITY, m),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/agent/pruefen/container` (Alias `/api/litellm/pruefen/container`): erreicht ein
/// Container den Endpunkt (Prüfmodus des Images)?
async fn endpunkt_pruefen_container(State(state): State<AppState>) -> Response {
    Json(state.container_pruefen().await).into_response()
}

async fn api_unbekannt() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "fehler": "Unbekannte Adresse" })),
    )
}

pub(crate) fn fehler(status: StatusCode, meldung: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: meldung.into(),
        }),
    )
        .into_response()
}

/// `GET /api/workspace`: der aktuelle Stand, Form von `cockpit_core::persist` plus `revision`.
async fn workspace(State(state): State<AppState>) -> Response {
    (
        [(CONTENT_TYPE, "application/json")],
        state.inner.live.schnappschuss(),
    )
        .into_response()
}

/// `POST /api/actions`: wendet die Aktion als „Du“ mit der Uhrzeit des Servers an. Wirkt sie
/// nach außen (Jira, GitHub), geht erst der Aufruf an den Dienst; scheitert er, ändert sich
/// nichts. Den Body lesen wir selbst, damit eine unbekannte Aktion eine deutsche Meldung bekommt.
async fn aktion(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(anfrage) = serde_json::from_slice::<ActionRequest>(&body) else {
        return fehler(
            StatusCode::BAD_REQUEST,
            "Die Anfrage ist keine bekannte Aktion",
        );
    };
    // Ein anderes Jira-Projekt: gleich abgleichen, damit dessen Tickets kommen. Das Ergebnis
    // erreicht die App über den Ereignisstrom.
    let projekt_gewechselt = matches!(
        anfrage.action,
        cockpit_core::Action::SelectJiraProject { .. }
    );
    match state.aktion_ausfuehren(anfrage.action).await {
        Ok(antwort) => {
            if projekt_gewechselt {
                let s = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = s.abgleichen().await {
                        tracing::warn!("Abgleich nach Projektwechsel: {e:?}");
                    }
                });
            }
            Json(antwort).into_response()
        }
        Err(e) => aktions_fehler(e),
    }
}

/// `POST /api/sync`: „jetzt abgleichen“.
async fn abgleichen(State(state): State<AppState>) -> Response {
    match state.abgleichen().await {
        Ok(antwort) => Json(antwort).into_response(),
        Err(e) => aktions_fehler(e),
    }
}

/// `GET /api/jira/projekte`: die Projekte für die Auswahl in der Inbox.
async fn jira_projekte(State(state): State<AppState>) -> Response {
    match state.jira_projekte().await {
        Ok(antwort) => Json(antwort).into_response(),
        Err(e) => aktions_fehler(e),
    }
}

fn aktions_fehler(e: AktionFehler) -> Response {
    match e {
        // Alle Ablehnungen der Regeln sind 422 – ohne einzelne Fehler aufzuzählen.
        AktionFehler::Regel(e) => fehler(StatusCode::UNPROCESSABLE_ENTITY, e.message()),
        AktionFehler::Plan(m) => fehler(StatusCode::UNPROCESSABLE_ENTITY, m),
        // Der Dienst hat abgelehnt oder ist nicht erreichbar.
        AktionFehler::Dienst(m) => fehler(StatusCode::BAD_GATEWAY, m),
        AktionFehler::Speichern(m) => {
            tracing::error!("Workspace nicht gesichert: {m}");
            fehler(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Stand konnte nicht gespeichert werden",
            )
        }
        AktionFehler::Intern => fehler(StatusCode::INTERNAL_SERVER_ERROR, "Interner Fehler"),
    }
}

/// `GET /api/events`: Server-Sent Events. Erst die aktuelle Revision, dann ein Ereignis
/// pro angewendeter Aktion. Wer zu langsam liest, bekommt die aktuelle Revision und lädt neu.
async fn ereignisse(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (rx, erstes) = state.inner.live.abonnieren();
    let weitere = BroadcastStream::new(rx).map(move |ereignis| {
        ereignis.unwrap_or_else(|_| ServerEvent {
            revision: state.inner.live.revision(),
            entries: Vec::new(),
        })
    });
    let strom = tokio_stream::once(erstes)
        .chain(weitere)
        .map(|e| Ok(Event::default().json_data(&e).unwrap_or_default()));
    Sse::new(strom).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

/// Baut den Router. `dist` ist der Ordner der gebauten Web-App.
pub fn router(state: AppState, dist: PathBuf) -> Router {
    let mut ports = vec![state.inner.port];
    ports.extend(state.inner.einstellungen.dev_port);
    let zugang = zugang::Zugang {
        dev: state.inner.einstellungen.dev_port.is_some(),
        ports,
    };
    Router::new()
        .route("/api/health", get(health))
        .route("/api/workspace", get(workspace))
        .route("/api/actions", post(aktion))
        .route("/api/sync", post(abgleichen))
        .route("/api/events", get(ereignisse))
        .route("/api/jira/projekte", get(jira_projekte))
        .route("/api/agent/modelle", get(agent_modelle))
        .route("/api/agent/pruefen", get(endpunkt_pruefen))
        .route("/api/litellm/pruefen", get(endpunkt_pruefen))
        .route(
            "/api/agent/pruefen/container",
            get(endpunkt_pruefen_container),
        )
        .route("/api/jira/pruefen", post(crate::einrichtung::jira_pruefen))
        .route(
            "/api/litellm/pruefen/container",
            get(endpunkt_pruefen_container),
        )
        .route(
            "/api/agent/image",
            get(agent_image_lesen).post(agent_image_bauen),
        )
        .route("/api/einrichtung", get(crate::einrichtung::lesen))
        .route("/api/konfiguration", put(crate::einrichtung::konfiguration))
        .route(
            "/api/schluessel/{name}",
            put(crate::einrichtung::schluessel).delete(crate::einrichtung::schluessel_loeschen),
        )
        .route("/api/oauth/{dienst}/start", get(crate::oauth::start))
        .route("/api/oauth/{dienst}/callback", get(crate::oauth::callback))
        .route(
            "/api/oauth/{dienst}/widerrufen",
            post(crate::oauth::widerrufen),
        )
        .route("/api/{*rest}", any(api_unbekannt))
        // Gilt nur für `/api`, nicht für die Dateien der App.
        .layer(axum::middleware::from_fn_with_state(
            zugang,
            zugang::schuetzen,
        ))
        .with_state(state)
        .fallback_service(ServeDir::new(dist))
}

#[cfg(test)]
mod tests;
