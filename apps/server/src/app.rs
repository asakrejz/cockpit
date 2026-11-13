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

use crate::access;
use crate::config::{AgentChoice, Config, Seed};
use crate::container::Cache;
use crate::db;
use crate::live::Live;
use crate::secrets::{self, SecretStore};
use crate::sync::{ActionFailure, Sources};
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
pub struct Settings {
    pub seed: Seed,
    /// `[agent] anbieter` und `[agent.endpunkt]`.
    pub agent: AgentChoice,
    /// Ersatz für `https://api.anthropic.com`, damit Tests nie die echte API erreichen.
    pub anthropic_api: Option<String>,
    /// `[agent] container`.
    pub container: Option<String>,
    /// `COCKPIT_CONTAINER` (geht der Datei vor).
    pub container_env: Option<String>,
    /// Wo nach `docker` und `podman` gesucht wird; `None` = `PATH` des Prozesses (Tests setzen es).
    pub search_path: Option<std::ffi::OsString>,
    /// `COCKPIT_DEV_PORT`: der Port von `trunk serve`, dessen Anfragen (Host, Origin) auch gelten.
    pub dev_port: Option<u16>,
    /// Die `config.toml`, in die der Einrichtungsassistent schreibt.
    pub config_path: Option<PathBuf>,
    /// `[agent] repo`: das Repo, in dem der Agent arbeitet.
    pub agent_repo: Option<String>,
    /// Ordner mit Containerfile und Läufer des Agenten.
    pub agent_dir: Option<PathBuf>,
    /// Basisadresse zum Klonen statt GitHub (nur Tests).
    pub git_base: Option<String>,
    /// Hier legt der Agent Klone und Aufträge ab (neben der Datenbank).
    pub data_dir: Option<PathBuf>,
    /// Feste Obergrenze in Sekunden für einen Lauf statt der Grenzen des Workspace (nur Tests).
    pub run_seconds: Option<u64>,
}

impl Settings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            seed: config.seed,
            agent: AgentChoice {
                provider: config.provider,
                endpoint: config.endpoint.clone(),
            },
            anthropic_api: None,
            container: config.container.clone(),
            container_env: config.container_env.clone(),
            search_path: None,
            dev_port: config.dev_port,
            config_path: config.config_path.clone(),
            agent_repo: config.agent_repo.clone(),
            agent_dir: Some(agent_dir_find(config)),
            git_base: config.git_base.clone(),
            data_dir: config.db.parent().map(PathBuf::from),
            run_seconds: None,
        }
    }
}

/// Der Ordner `agent` mit Containerfile und Läufer: `[agent] dir` bzw. `COCKPIT_AGENT_DIR`, sonst
/// neben der gebauten App (`<dist>/../../agent`, so liegt er im Repo und im Release-Archiv),
/// sonst `./apps/agent`.
fn agent_dir_find(config: &Config) -> PathBuf {
    if let Some(d) = &config.agent_dir {
        return d.clone();
    }
    let next_to_dist = config.dist.join("..").join("..").join("agent");
    if next_to_dist.join("Containerfile").is_file() {
        return next_to_dist;
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
    pub(crate) sign_ins: Mutex<cockpit_integrations::oauth::SignIns>,
    /// Nur eine Auffrischung der OAuth-Tokens gleichzeitig (Refresh-Tokens drehen sich).
    pub(crate) refresh: tokio::sync::Mutex<()>,
    /// Jira und GitHub, wie konfiguriert (ob sie laufen, hängt noch vom Schlüssel ab).
    /// Wird beim Speichern im Einrichtungsassistenten ausgetauscht (ohne Neustart).
    pub(crate) sources: RwLock<Arc<Sources>>,
    /// Anbieter und Endpunkt des Agenten, ebenfalls austauschbar.
    pub(crate) agent_choice: RwLock<AgentChoice>,
    /// Lesen, Ändern und Schreiben von `config.toml` gehen nacheinander.
    pub(crate) config_lock: Mutex<()>,
    /// Schreibzugriffe nach außen und das Übernehmen eines Abgleichs gehen nacheinander.
    pub(crate) lock: tokio::sync::Mutex<()>,
    /// Zählt erfolgreiche Schreibzugriffe: Ein Abruf, der währenddessen begann, ist veraltet.
    pub(crate) written: AtomicU64,
    /// Nur ein Abgleich gleichzeitig.
    pub(crate) sync: tokio::sync::Mutex<()>,
    pub(crate) settings: Settings,
    /// Ergebnis der Suche nach der Container-Laufzeit, kurz gemerkt.
    pub(crate) container: Cache,
    /// Läufe des echten Agenten (Container) und der Bau des Images.
    pub(crate) agent: crate::agent_ops::Runs,
}

impl AppState {
    /// Wie [`AppState::mit_port`], mit dem Standard-Port (für Tests).
    #[cfg(test)]
    pub fn new(db: Connection, secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_port(db, secrets, crate::config::DEFAULT_PORT)
    }

    /// Lädt dabei den gespeicherten Workspace (sonst die Beispieldaten).
    #[cfg(test)]
    pub fn with_port(db: Connection, secrets: Arc<dyn SecretStore>, port: u16) -> Self {
        Self::with_sources(db, secrets, port, Sources::default())
    }

    /// Wie [`AppState::mit_port`], mit Jira und GitHub für den Abgleich. Die Tests starten mit
    /// den Beispieldaten, der Server selbst mit leerem Workspace ([`AppState::neu`]).
    #[cfg(test)]
    pub fn with_sources(
        db: Connection,
        secrets: Arc<dyn SecretStore>,
        port: u16,
        sources: Sources,
    ) -> Self {
        let settings = Settings {
            seed: Seed::Demo,
            // Tests suchen nirgends nach Container-Programmen: gleiches Ergebnis auf jedem Rechner.
            search_path: Some(std::ffi::OsString::new()),
            ..Settings::default()
        };
        Self::create(db, secrets, port, sources, settings)
    }

    /// Der Zustand des Servers. Ein gespeicherter Workspace wird geladen, sonst gilt der Seed.
    pub fn create(
        db: Connection,
        secrets: Arc<dyn SecretStore>,
        port: u16,
        sources: Sources,
        settings: Settings,
    ) -> Self {
        let live = Live::laden(&db, settings.seed);
        Self {
            inner: Arc::new(Inner {
                db: Mutex::new(db),
                secrets,
                live,
                port,
                sign_ins: crate::oauth::new_sign_ins(),
                refresh: tokio::sync::Mutex::new(()),
                sources: RwLock::new(Arc::new(sources)),
                agent_choice: RwLock::new(settings.agent.clone()),
                config_lock: Mutex::new(()),
                lock: tokio::sync::Mutex::new(()),
                written: AtomicU64::new(0),
                sync: tokio::sync::Mutex::new(()),
                settings,
                container: Cache::default(),
                agent: crate::agent_ops::Runs::default(),
            }),
        }
    }

    /// Jira und GitHub, wie zuletzt konfiguriert.
    pub(crate) fn sources(&self) -> Arc<Sources> {
        match self.inner.sources.read() {
            Ok(q) => q.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Anbieter und Endpunkt des Agenten, wie zuletzt konfiguriert.
    pub(crate) fn agent_choice(&self) -> AgentChoice {
        match self.inner.agent_choice.read() {
            Ok(l) => l.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Übernimmt eine neu geschriebene Konfiguration ohne Neustart: Quellen, OAuth-Apps und
    /// Agent-Endpunkt. Port, Datenbank, Abgleich-Takt und Container-Wahl bleiben, wie sie sind.
    pub(crate) fn config_apply(&self, c: &Config) {
        let alt = self.sources();
        let updated = Sources {
            github: c.github.clone(),
            jira: c.jira.clone(),
            github_oauth: c.github_oauth.clone(),
            atlassian: c.atlassian.clone(),
            endpoints: alt.endpoints.clone(),
            interval: alt.interval,
        };
        match self.inner.sources.write() {
            Ok(mut q) => *q = Arc::new(updated),
            Err(v) => *v.into_inner() = Arc::new(updated),
        }
        let choice = AgentChoice {
            provider: c.provider,
            endpoint: c.endpoint.clone(),
        };
        match self.inner.agent_choice.write() {
            Ok(mut l) => *l = choice,
            Err(v) => *v.into_inner() = choice,
        }
    }

    /// Ein Takt der Aufgaben im Hintergrund. Der simulierte Agent macht nur mit, wenn der Server
    /// mit dem Seed `demo` läuft; sonst arbeitet der echte Agent in Containern. Blockierend.
    pub fn takt(&self) {
        self.inner
            .live
            .takt(&self.inner.db, now_ms(), !self.agent_real());
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Startet die Hintergrundarbeit des Agenten im Server: im Modus „echt“ der Aufpasser für die
/// Container, mit dem Seed `demo` der Takt des simulierten Agenten (statt im Browser).
pub fn agent_takt_start(state: AppState) {
    if state.agent_real() {
        crate::agent_ops::watcher_start(state);
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
pub struct Connections {
    pub github: bool,
    pub jira: bool,
}

/// Art und Stand der Anmeldung je Dienst – für „Mit … anmelden“ und „Trennen“.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SignIn {
    pub github: ServiceStatus,
    pub jira: ServiceStatus,
}

/// Zustand des Abgleichs je Quelle: `aus` (nicht eingerichtet oder ohne Schlüssel), `wartet`
/// (noch kein Abruf), `ok`, `nicht-angemeldet`, `keine-berechtigung`, `kontingent`,
/// `offline` oder `fehler`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SyncState {
    pub github: String,
    pub jira: String,
    pub interval_s: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    pub connections: Connections,
    pub sign_in: SignIn,
    pub sync: SyncState,
    /// Anbieter, Adresse, Modell und ob ein Key liegt – nie der Key.
    pub endpoint: EndpointStatus,
    pub container: ContainerStatus,
    /// Ob der echte Agent loslaufen kann und was fehlt.
    pub agent: AgentStatus,
}

impl AppState {
    /// Blockierend (Datenbank, Schlüsselbund) – nicht direkt in einem async-Handler aufrufen.
    fn health(&self) -> Health {
        let names = match self.inner.db.lock() {
            Ok(conn) => db::connections(&conn).unwrap_or_else(|e| {
                tracing::warn!("Verbindungen konnten nicht gelesen werden: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        // Ein Token (Datenbank-Eintrag **und** Schlüssel) oder die Anmeldung per OAuth.
        let set_up = |name: &str| {
            names.iter().any(|n| n == name)
                && self.inner.secrets.has(name).unwrap_or_else(|e| {
                    tracing::warn!("Schlüssel „{name}“ nicht prüfbar: {e}");
                    false
                })
        };
        let github = self.service_status(Service::GitHub, set_up(secrets::GITHUB));
        // Jira per Token braucht Adresse und E-Mail aus der Konfiguration (Eintrag in der Datenbank).
        let jira = self.service_status(Service::Atlassian, set_up(secrets::JIRA));
        let connections = Connections {
            github: github.connected,
            jira: jira.connected,
        };
        let has_key = |name: &str| self.inner.secrets.has(name).unwrap_or(false);
        let ws = self.inner.live.copy();
        let state = |active: bool, source: cockpit_core::Source| {
            if !active {
                return "off".to_string();
            }
            ws.sync
                .state_of(source)
                .map_or("waiting", cockpit_core::SourceState::id)
                .to_string()
        };
        let q = self.sources();
        let choice = self.agent_choice();
        let endpoint = EndpointStatus {
            configured: choice.endpoint.is_some(),
            provider: choice.provider,
            url: choice.endpoint.as_ref().and_then(|e| e.url.clone()),
            model: choice.endpoint.as_ref().and_then(|e| e.model.clone()),
            key: has_key(secrets::AGENT_KEY) || has_key(secrets::LITELLM),
        };
        let container = self.container_status();
        let agent = self.agent_status();
        Health {
            status: "ok",
            version: VERSION,
            sync: SyncState {
                github: state(
                    q.github.as_ref().is_some_and(|g| !g.repos.is_empty())
                        && (github.kind == AuthKind::OAuth || has_key(secrets::GITHUB)),
                    cockpit_core::Source::GitHub,
                ),
                jira: state(
                    q.jira.is_some() && (jira.kind == AuthKind::OAuth || has_key(secrets::JIRA)),
                    cockpit_core::Source::Jira,
                ),
                interval_s: q.interval.as_secs(),
            },
            connections,
            sign_in: SignIn { github, jira },
            endpoint,
            container,
            agent,
        }
    }
}

async fn health(State(state): State<AppState>) -> Result<Json<Health>, StatusCode> {
    tokio::task::spawn_blocking(move || state.health())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Der Key des Endpunkts aus dem Schlüsselbund: `agent-key`, ersatzweise das ältere `litellm`.
/// Blockierend.
pub(crate) fn agent_key_read(
    secrets: &dyn crate::secrets::SecretStore,
) -> Result<Option<cockpit_integrations::Secret>, crate::secrets::SecretError> {
    match secrets.get(secrets::AGENT_KEY)? {
        Some(k) => Ok(Some(k)),
        None => secrets.get(secrets::LITELLM),
    }
}

impl AppState {
    /// Wohin die Aufrufe an den Endpunkt gehen: die eingetragene Adresse, sonst direkt Anthropic.
    pub(crate) fn endpoint_target(&self) -> cockpit_integrations::endpoint::Target {
        let choice = self.agent_choice();
        cockpit_integrations::endpoint::Target::from(
            choice.endpoint.as_ref().and_then(|e| e.url.as_deref()),
            self.inner.settings.anthropic_api.as_deref(),
        )
    }

    /// Der Key des Endpunkts. `Err` ist schon eine fertige Antwort.
    async fn endpoint_key(&self) -> Result<Option<cockpit_integrations::Secret>, Box<Response>> {
        let secrets = self.inner.secrets.clone();
        match tokio::task::spawn_blocking(move || agent_key_read(secrets.as_ref())).await {
            Ok(Ok(k)) => Ok(k),
            Ok(Err(e)) => {
                tracing::warn!("Key des Agent-Endpunkts nicht lesbar: {e}");
                Err(Box::new(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Der Schlüsselbund ist nicht lesbar",
                )))
            }
            Err(_) => Err(Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response())),
        }
    }
}

/// `GET /api/agent/check`: ruft nur `GET <url>/v1/models` mit dem
/// Key auf und meldet, ob der Endpunkt erreichbar ist, den Key annimmt und das Modell anbietet.
/// Nie der Key.
async fn endpoint_check(State(state): State<AppState>) -> Response {
    let choice = state.agent_choice();
    let Some(e) = choice.endpoint.clone() else {
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
    let key = match state.endpoint_key().await {
        Ok(k) => k,
        Err(answer) => return *answer,
    };
    Json(
        cockpit_integrations::endpoint::verify(
            &state.endpoint_target(),
            key.as_ref(),
            e.model.as_deref(),
        )
        .await,
    )
    .into_response()
}

/// `GET /api/agent/models`: die Modell-IDs des Endpunkts (nur IDs, sortiert) für das Drop-down.
/// Der Key kommt aus dem Schlüsselbund und verlässt das Backend nie.
async fn agent_models(State(state): State<AppState>) -> Response {
    let key = match state.endpoint_key().await {
        Ok(k) => k,
        Err(answer) => return *answer,
    };
    Json(cockpit_integrations::endpoint::models_load(&state.endpoint_target(), key.as_ref()).await)
        .into_response()
}

/// `GET /api/agent/image`: Stand von „Image bauen“ (läuft, vorhanden, letzte Zeilen, Fehler).
async fn agent_image_read(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_state()).await {
        Ok(b) => Json(b).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/agent/image`: baut das Agent-Image im Hintergrund (`docker build` bzw. `podman build`).
/// Der Fortschritt steht in `GET /api/agent/image`.
async fn agent_image_build(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_build()).await {
        Ok(Ok(b)) => Json(b).into_response(),
        Ok(Err(m)) => error(StatusCode::UNPROCESSABLE_ENTITY, m),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/agent/check/container`: erreicht ein
/// Container den Endpunkt (Prüfmodus des Images)?
async fn endpoint_check_container(State(state): State<AppState>) -> Response {
    Json(state.container_check().await).into_response()
}

async fn api_unknown() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "Unbekannte Adresse" })),
    )
}

pub(crate) fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

/// `GET /api/workspace`: der aktuelle Stand, Form von `cockpit_core::persist` plus `revision`.
async fn workspace(State(state): State<AppState>) -> Response {
    (
        [(CONTENT_TYPE, "application/json")],
        state.inner.live.snapshot(),
    )
        .into_response()
}

/// `POST /api/actions`: wendet die Aktion als „Du“ mit der Uhrzeit des Servers an. Wirkt sie
/// nach außen (Jira, GitHub), geht erst der Aufruf an den Dienst; scheitert er, ändert sich
/// nichts. Den Body lesen wir selbst, damit eine unbekannte Aktion eine deutsche Meldung bekommt.
async fn action(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<ActionRequest>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "Die Anfrage ist keine bekannte Aktion",
        );
    };
    // Ein anderes Jira-Projekt: gleich abgleichen, damit dessen Tickets kommen. Das Ergebnis
    // erreicht die App über den Ereignisstrom.
    let project_switched = matches!(
        request.action,
        cockpit_core::Action::SelectJiraProject { .. }
    );
    match state.action_run(request.action).await {
        Ok(answer) => {
            if project_switched {
                let s = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = s.sync().await {
                        tracing::warn!("Abgleich nach Projektwechsel: {e:?}");
                    }
                });
            }
            Json(answer).into_response()
        }
        Err(e) => action_error(e),
    }
}

/// `POST /api/sync`: „jetzt abgleichen“.
async fn sync(State(state): State<AppState>) -> Response {
    match state.sync().await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

/// `GET /api/jira/projects`: die Projekte für die Auswahl in der Inbox.
async fn jira_projects(State(state): State<AppState>) -> Response {
    match state.jira_projects().await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

fn action_error(e: ActionFailure) -> Response {
    match e {
        // Alle Ablehnungen der Regeln sind 422 – ohne einzelne Fehler aufzuzählen.
        ActionFailure::Rule(e) => error(StatusCode::UNPROCESSABLE_ENTITY, e.message()),
        ActionFailure::Plan(m) => error(StatusCode::UNPROCESSABLE_ENTITY, m),
        // Der Dienst hat abgelehnt oder ist nicht erreichbar.
        ActionFailure::Service(m) => error(StatusCode::BAD_GATEWAY, m),
        ActionFailure::Save(m) => {
            tracing::error!("Workspace nicht gesichert: {m}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Stand konnte nicht gespeichert werden",
            )
        }
        ActionFailure::Intern => error(StatusCode::INTERNAL_SERVER_ERROR, "Interner Fehler"),
    }
}

/// `GET /api/events`: Server-Sent Events. Erst die aktuelle Revision, dann ein Ereignis
/// pro angewendeter Aktion. Wer zu langsam liest, bekommt die aktuelle Revision und lädt neu.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (rx, first) = state.inner.live.subscribe();
    let more = BroadcastStream::new(rx).map(move |event| {
        event.unwrap_or_else(|_| ServerEvent {
            revision: state.inner.live.revision(),
            entries: Vec::new(),
        })
    });
    let strom = tokio_stream::once(first)
        .chain(more)
        .map(|e| Ok(Event::default().json_data(&e).unwrap_or_default()));
    Sse::new(strom).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

/// Baut den Router. `dist` ist der Ordner der gebauten Web-App.
pub fn router(state: AppState, dist: PathBuf) -> Router {
    let mut ports = vec![state.inner.port];
    ports.extend(state.inner.settings.dev_port);
    let access = access::Access {
        dev: state.inner.settings.dev_port.is_some(),
        ports,
    };
    Router::new()
        .route("/api/health", get(health))
        .route("/api/workspace", get(workspace))
        .route("/api/actions", post(action))
        .route("/api/sync", post(sync))
        .route("/api/events", get(events))
        .route("/api/jira/projects", get(jira_projects))
        .route("/api/agent/models", get(agent_models))
        .route("/api/agent/check", get(endpoint_check))
        .route("/api/agent/check/container", get(endpoint_check_container))
        .route("/api/jira/check", post(crate::setup::jira_check))
        .route(
            "/api/agent/image",
            get(agent_image_read).post(agent_image_build),
        )
        .route("/api/setup", get(crate::setup::read))
        .route("/api/config", put(crate::setup::config))
        .route(
            "/api/secrets/{name}",
            put(crate::setup::key).delete(crate::setup::key_delete),
        )
        .route("/api/oauth/{service}/start", get(crate::oauth::start))
        .route("/api/oauth/{service}/callback", get(crate::oauth::callback))
        .route("/api/oauth/{service}/revoke", post(crate::oauth::revoke))
        .route("/api/{*rest}", any(api_unknown))
        // Gilt nur für `/api`, nicht für die Dateien der App.
        .layer(axum::middleware::from_fn_with_state(
            access,
            access::protect,
        ))
        .with_state(state)
        .fallback_service(ServeDir::new(dist))
}

#[cfg(test)]
mod tests;
