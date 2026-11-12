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
use crate::config::{Config, LitellmKonfig, Seed};
use crate::container::Zwischenspeicher;
use crate::db;
use crate::live::Live;
use crate::secrets::{self, SecretStore};
use crate::zugang;
use cockpit_core::api::{AgentStatus, ContainerStatus, LitellmStatus};
use cockpit_core::{AuthKind, ServiceStatus};
use cockpit_integrations::oauth::Dienst;

/// So oft macht der simulierte Agent einen Schritt (wie im Demo-Modus der App).
pub const AGENT_TAKT: Duration = Duration::from_millis(2200);
/// So oft kommt ein Lebenszeichen auf der Ereignis-Verbindung.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Einstellungen aus `config.toml` und Umgebung, die nicht Jira oder GitHub betreffen.
#[derive(Debug, Clone, Default)]
pub struct Einstellungen {
    pub seed: Seed,
    pub litellm: Option<LitellmKonfig>,
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
            litellm: config.litellm.clone(),
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
/// sonst `./agent`.
fn agent_ordner_finden(config: &Config) -> PathBuf {
    if let Some(d) = &config.agent_dir {
        return d.clone();
    }
    let neben_dist = config.dist.join("..").join("..").join("agent");
    if neben_dist.join("Containerfile").is_file() {
        return neben_dist;
    }
    PathBuf::from("agent")
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
    pub(crate) anmeldungen: Mutex<cockpit_integrations::oauth::Anmeldungen>,
    /// Nur eine Auffrischung der OAuth-Tokens gleichzeitig (Refresh-Tokens drehen sich).
    pub(crate) auffrischen: tokio::sync::Mutex<()>,
    /// Jira und GitHub, wie konfiguriert (ob sie laufen, hängt noch vom Schlüssel ab).
    /// Wird beim Speichern im Einrichtungsassistenten ausgetauscht (ohne Neustart).
    pub(crate) quellen: RwLock<Arc<Quellen>>,
    /// Der LiteLLM-Proxy, ebenfalls austauschbar.
    pub(crate) litellm: RwLock<Option<LitellmKonfig>>,
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
                litellm: RwLock::new(einstellungen.litellm.clone()),
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

    /// Der LiteLLM-Proxy, wie zuletzt konfiguriert.
    pub(crate) fn litellm(&self) -> Option<LitellmKonfig> {
        match self.inner.litellm.read() {
            Ok(l) => l.clone(),
            Err(vergiftet) => vergiftet.into_inner().clone(),
        }
    }

    /// Übernimmt eine neu geschriebene Konfiguration ohne Neustart: Quellen, OAuth-Apps und
    /// LiteLLM. Port, Datenbank, Abgleich-Takt und Container-Wahl bleiben, wie sie sind.
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
        match self.inner.litellm.write() {
            Ok(mut l) => *l = c.litellm.clone(),
            Err(v) => *v.into_inner() = c.litellm.clone(),
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
    /// Adresse, Modell und ob ein Key liegt – nie der Key.
    pub litellm: LitellmStatus,
    pub container: ContainerStatus,
    /// Ob der echte Agent loslaufen kann und was fehlt.
    pub agent: AgentStatus,
}

impl AppState {
    /// Blockierend (Datenbank, Schlüsselbund) – nicht direkt in einem async-Handler aufrufen.
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
        let github = self.dienst_status(Dienst::GitHub, eingerichtet(secrets::GITHUB));
        // Jira per Token braucht Adresse und E-Mail aus der Konfiguration (Eintrag in der Datenbank).
        let jira = self.dienst_status(Dienst::Atlassian, eingerichtet(secrets::JIRA));
        let verbindungen = Verbindungen {
            github: github.verbunden,
            jira: jira.verbunden,
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
        let proxy = self.litellm();
        let litellm = LitellmStatus {
            konfiguriert: proxy.is_some(),
            url: proxy.as_ref().map(|l| l.url.clone()),
            modell: proxy.as_ref().and_then(|l| l.modell.clone()),
            key: hat_schluessel(secrets::LITELLM),
        };
        let container = self.container_status();
        let agent = self.agent_status();
        Health {
            status: "ok",
            version: VERSION,
            abgleich: AbgleichZustand {
                github: zustand(
                    q.github.as_ref().is_some_and(|g| !g.repos.is_empty())
                        && (github.art == AuthKind::OAuth || hat_schluessel(secrets::GITHUB)),
                    cockpit_core::Source::GitHub,
                ),
                jira: zustand(
                    q.jira.is_some()
                        && (jira.art == AuthKind::OAuth || hat_schluessel(secrets::JIRA)),
                    cockpit_core::Source::Jira,
                ),
                intervall_s: q.intervall.as_secs(),
            },
            verbindungen,
            anmeldung: Anmeldung { github, jira },
            litellm,
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

/// `GET /api/litellm/pruefen`: ruft nur `GET <url>/v1/models` mit dem Key auf und meldet, ob der
/// Proxy erreichbar ist, den Key annimmt und das Modell anbietet. Nie der Key.
async fn litellm_pruefen(State(state): State<AppState>) -> Response {
    let Some(l) = state.litellm() else {
        return Json(cockpit_core::api::LitellmPruefung {
            hinweis: Some(
                "Der Proxy ist nicht eingerichtet. Trage url (und modell) unter [litellm] in config.toml ein."
                    .into(),
            ),
            ..Default::default()
        })
        .into_response();
    };
    let secrets = state.inner.secrets.clone();
    let key = tokio::task::spawn_blocking(move || secrets.get(secrets::LITELLM)).await;
    let key = match key {
        Ok(Ok(k)) => k,
        Ok(Err(e)) => {
            tracing::warn!("LiteLLM-Key nicht lesbar: {e}");
            return fehler(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Schlüsselbund ist nicht lesbar",
            );
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    Json(cockpit_integrations::litellm::pruefen(&l.url, key.as_ref(), l.modell.as_deref()).await)
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

/// `GET /api/litellm/pruefen/container`: erreicht ein Container den Proxy (Prüfmodus des Images)?
async fn litellm_pruefen_container(State(state): State<AppState>) -> Response {
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
            fehler: meldung.into(),
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
            eintraege: Vec::new(),
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
        .route("/api/litellm/pruefen", get(litellm_pruefen))
        .route(
            "/api/litellm/pruefen/container",
            get(litellm_pruefen_container),
        )
        .route(
            "/api/agent/image",
            get(agent_image_lesen).post(agent_image_bauen),
        )
        .route("/api/einrichtung", get(crate::einrichtung::lesen))
        .route("/api/konfiguration", put(crate::einrichtung::konfiguration))
        .route(
            "/api/schluessel/{name}",
            put(crate::einrichtung::schluessel),
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
mod tests {
    use super::*;
    use std::net::SocketAddr;

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::Request;
    use cockpit_core::Actor;
    use cockpit_integrations::Secret;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::secrets::MemoryStore;

    fn zustand(secrets: Arc<MemoryStore>) -> AppState {
        let conn = db::open_im_speicher().unwrap();
        db::verbindung_setzen(&conn, "github", "https://api.github.com", None).unwrap();
        db::verbindung_setzen(&conn, "jira", "https://x.atlassian.net", Some("a@b.de")).unwrap();
        AppState::new(conn, secrets)
    }

    /// Eine Anfrage, wie sie der Browser von `127.0.0.1:8787` schickt.
    fn anfrage(methode: &str, pfad: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(methode)
            .uri(pfad)
            .header("host", "127.0.0.1:8787")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
    }

    async fn sende(app: Router, req: Request<Body>) -> (StatusCode, String) {
        let antwort = app.oneshot(req).await.unwrap();
        let status = antwort.status();
        let body = antwort.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn hole(app: Router, pfad: &str) -> (StatusCode, String) {
        sende(app, anfrage("GET", pfad).body(Body::empty()).unwrap()).await
    }

    async fn schicke(app: Router, json: &str) -> (StatusCode, String) {
        let req = anfrage("POST", "/api/actions")
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap();
        sende(app, req).await
    }

    #[tokio::test]
    async fn health_ohne_schluessel() {
        let app = router(
            zustand(Arc::new(MemoryStore::new())),
            PathBuf::from("/gibt/es/nicht"),
        );
        let (status, body) = hole(app, "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], VERSION);
        assert_eq!(json["verbindungen"]["github"], false);
        assert_eq!(json["verbindungen"]["jira"], false);
    }

    #[tokio::test]
    async fn health_zeigt_nur_ja_nein_nie_den_schluessel() {
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .set(secrets::GITHUB, &Secret::new("ghp_streng_geheim"))
            .unwrap();
        let app = router(zustand(secrets), PathBuf::from("/gibt/es/nicht"));
        let (status, body) = hole(app, "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("ghp_streng_geheim"), "{body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["verbindungen"]["github"], true);
        assert_eq!(json["verbindungen"]["jira"], false);
    }

    #[tokio::test]
    async fn schluessel_ohne_verbindung_zaehlt_nicht() {
        let secrets = Arc::new(MemoryStore::new());
        secrets.set(secrets::JIRA, &Secret::new("t")).unwrap();
        let state = AppState::new(db::open_im_speicher().unwrap(), secrets);
        let h = state.health();
        assert_eq!(
            h.verbindungen,
            Verbindungen {
                github: false,
                jira: false
            }
        );
    }

    #[tokio::test]
    async fn unbekannte_api_adresse_ist_404_json() {
        let app = router(
            zustand(Arc::new(MemoryStore::new())),
            PathBuf::from("/gibt/es/nicht"),
        );
        let (status, body) = hole(app, "/api/gibt-es-nicht").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("fehler"));
    }

    #[tokio::test]
    async fn liefert_die_app_als_statische_dateien() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.html"), "<h1>Cockpit</h1>").unwrap();
        std::fs::write(tmp.path().join("app.js"), "console.log(1)").unwrap();
        let app = || {
            router(
                zustand(Arc::new(MemoryStore::new())),
                tmp.path().to_path_buf(),
            )
        };

        let (status, body) = hole(app(), "/").await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::OK, "<h1>Cockpit</h1>")
        );
        let (status, body) = hole(app(), "/app.js").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "console.log(1)"));
        let (status, _) = hole(app(), "/fehlt.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    fn app_ohne_dist() -> Router {
        router(
            zustand(Arc::new(MemoryStore::new())),
            PathBuf::from("/gibt/es/nicht"),
        )
    }

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    /// Interne ID des ersten Eintrags der Beispieldaten.
    fn erste_id() -> String {
        cockpit_core::demo::workspace().items[0].id.clone()
    }

    fn mark_done(id: &str) -> String {
        format!(r#"{{"action":{{"MarkDone":{{"item":"{id}"}}}}}}"#)
    }

    // ---------- GET /api/workspace ----------

    #[tokio::test]
    async fn workspace_startet_mit_den_demo_daten() {
        let (status, body) = hole(app_ohne_dist(), "/api/workspace").await;
        assert_eq!(status, StatusCode::OK);
        let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
        assert_eq!(ws, cockpit_core::demo::workspace());
        assert_eq!(revision, 0);
    }

    // ---------- POST /api/actions ----------

    #[tokio::test]
    async fn aktion_aendert_den_stand_und_zaehlt_die_revision() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        let id = erste_id();

        let (status, body) = schicke(app(), &mark_done(&id)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let antwort = json(&body);
        assert_eq!(antwort["revision"], 1);
        assert_eq!(antwort["protokolliert"], 1);
        assert!(!antwort["zusammenfassung"].as_str().unwrap().is_empty());

        let (_, body) = hole(app(), "/api/workspace").await;
        let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
        assert_eq!(revision, 1);
        assert!(ws.item(&id).unwrap().read);
        // Der Server wendet als „Du“ an, mit seiner Uhr.
        let letzter = ws.log.last().unwrap();
        assert_eq!(letzter.actor, Actor::Du);
        assert!(letzter.at_ms > 1_600_000_000_000);
    }

    #[tokio::test]
    async fn regelverstoss_ist_422_mit_deutscher_meldung_und_aendert_nichts() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        let vorher = hole(app(), "/api/workspace").await.1;

        let (status, body) =
            schicke(app(), r#"{"action":{"MarkDone":{"item":"gibt-es-nicht"}}}"#).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            json(&body)["fehler"]
                .as_str()
                .unwrap()
                .contains("gibt es nicht")
        );
        assert_eq!(hole(app(), "/api/workspace").await.1, vorher);
    }

    #[tokio::test]
    async fn review_senden_mit_offenen_vorschlaegen_ist_4xx() {
        let ws = cockpit_core::demo::workspace();
        let number = ws
            .reviews
            .iter()
            .find(|r| r.suggestions.iter().any(|s| s.decision.is_none()))
            .expect("Beispieldaten haben ein Review mit offenen Vorschlägen")
            .number;
        let body = format!(r#"{{"action":{{"SendReview":{{"review":{number},"comment":""}}}}}}"#);
        let (status, antwort) = schicke(app_ohne_dist(), &body).await;
        assert!(status.is_client_error(), "{status}");
        assert!(
            json(&antwort)["fehler"]
                .as_str()
                .is_some_and(|m| !m.is_empty())
        );
    }

    #[tokio::test]
    async fn mergen_nur_mit_erfuellten_bedingungen() {
        // Über die API kommt jede Aktion als „Du“. Dass ein Agent nie merged, prüft
        // `Workspace::apply` selbst (Actor); hier greifen die Bedingungen fürs Mergen.
        let body = r#"{"action":{"MergePr":{"review":87,"method":"Squash"}}}"#;
        let (status, antwort) = schicke(app_ohne_dist(), body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{antwort}");
        assert!(!json(&antwort)["fehler"].as_str().unwrap().is_empty());
        // Formen, die es nicht gibt, bleiben 400.
        for body in [
            r#"{"action":{"Merge":{"review":87}}}"#,
            r#"{"action":"MergePullRequest"}"#,
        ] {
            let (status, antwort) = schicke(app_ohne_dist(), body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert!(
                json(&antwort)["fehler"]
                    .as_str()
                    .unwrap()
                    .contains("Aktion")
            );
        }
    }

    #[tokio::test]
    async fn kaputter_body_ist_400() {
        for body in ["", "kein json", "{}", r#"{"action":"GibtEsNicht"}"#] {
            let (status, antwort) = schicke(app_ohne_dist(), body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
            assert!(antwort.contains("fehler"));
        }
    }

    // ---------- Sicherheit ----------

    #[tokio::test]
    async fn post_ohne_json_content_type_ist_415() {
        for typ in [
            None,
            Some("text/plain"),
            Some("application/x-www-form-urlencoded"),
        ] {
            let mut b = anfrage("POST", "/api/actions");
            if let Some(t) = typ {
                b = b.header("content-type", t);
            }
            let req = b.body(Body::from(mark_done(&erste_id()))).unwrap();
            let (status, _) = sende(app_ohne_dist(), req).await;
            assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{typ:?}");
        }
    }

    #[tokio::test]
    async fn post_mit_fremder_herkunft_ist_403_und_aendert_nichts() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        let vorher = hole(app(), "/api/workspace").await.1;
        let req = anfrage("POST", "/api/actions")
            .header("content-type", "application/json")
            .header("origin", "https://evil.example")
            .body(Body::from(mark_done(&erste_id())))
            .unwrap();
        let (status, _) = sende(app(), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(hole(app(), "/api/workspace").await.1, vorher);

        // Gleiche Herkunft geht.
        let req = anfrage("POST", "/api/actions")
            .header("content-type", "application/json")
            .header("origin", "http://localhost:8787")
            .body(Body::from(mark_done(&erste_id())))
            .unwrap();
        assert_eq!(sende(app(), req).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn fremder_host_ist_403_auf_allen_api_routen() {
        for (methode, pfad) in [
            ("GET", "/api/health"),
            ("GET", "/api/workspace"),
            ("GET", "/api/events"),
            ("POST", "/api/actions"),
        ] {
            let req = Request::builder()
                .method(methode)
                .uri(pfad)
                .header("host", "evil.example:8787")
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
                .body(Body::from(mark_done(&erste_id())))
                .unwrap();
            let (status, _) = sende(app_ohne_dist(), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{methode} {pfad}");
        }
    }

    #[tokio::test]
    async fn fremde_gegenstelle_ist_403() {
        let req = Request::get("/api/workspace")
            .header("host", "127.0.0.1:8787")
            .extension(ConnectInfo(SocketAddr::from(([192, 168, 0, 7], 1))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(sende(app_ohne_dist(), req).await.0, StatusCode::FORBIDDEN);
        // Ohne bekannte Gegenstelle wird abgewiesen, nicht durchgelassen.
        let req = Request::get("/api/workspace")
            .header("host", "127.0.0.1:8787")
            .body(Body::empty())
            .unwrap();
        assert_eq!(sende(app_ohne_dist(), req).await.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn host_wird_gegen_den_konfigurierten_port_geprueft() {
        let state = AppState::mit_port(
            db::open_im_speicher().unwrap(),
            Arc::new(MemoryStore::new()),
            9000,
        );
        let app = || router(state.clone(), PathBuf::from("/x"));
        assert_eq!(hole(app(), "/api/health").await.0, StatusCode::FORBIDDEN);
        let req = Request::get("/api/health")
            .header("host", "localhost:9000")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(sende(app(), req).await.0, StatusCode::OK);
    }

    // ---------- GET /api/events ----------

    /// Liest vom Ereignisstrom, bis `n` Ereignisse (`data:`-Zeilen) da sind.
    async fn lese_ereignisse(body: Body, n: usize) -> Vec<serde_json::Value> {
        use http_body_util::BodyExt;
        let mut body = body;
        let mut text = String::new();
        let mut daten = Vec::new();
        while daten.len() < n {
            let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
                .await
                .expect("Ereignis kam nicht")
                .expect("Strom zu Ende")
                .unwrap();
            if let Some(chunk) = frame.data_ref() {
                text.push_str(&String::from_utf8_lossy(chunk));
            }
            daten = text
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(|l| json(l.trim()))
                .collect();
        }
        daten
    }

    #[tokio::test]
    async fn ereignisse_melden_revision_und_neue_eintraege() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));

        let antwort = app()
            .oneshot(anfrage("GET", "/api/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(antwort.status(), StatusCode::OK);
        assert_eq!(
            antwort.headers()[CONTENT_TYPE].to_str().unwrap(),
            "text/event-stream"
        );

        let (status, _) = schicke(app(), &mark_done(&erste_id())).await;
        assert_eq!(status, StatusCode::OK);

        let ereignisse = lese_ereignisse(antwort.into_body(), 2).await;
        // Erst der Stand beim Verbinden, dann die Aktion.
        assert_eq!(ereignisse[0]["revision"], 0);
        assert_eq!(ereignisse[0]["eintraege"], serde_json::json!([]));
        assert_eq!(ereignisse[1]["revision"], 1);
        let eintraege = ereignisse[1]["eintraege"].as_array().unwrap();
        assert_eq!(eintraege.len(), 1);
        assert_eq!(eintraege[0]["actor"], "Du");
    }

    #[tokio::test]
    async fn fehlgeschlagene_aktion_sendet_kein_ereignis() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        let antwort = app()
            .oneshot(anfrage("GET", "/api/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, _) = schicke(app(), r#"{"action":{"MarkDone":{"item":"nein"}}}"#).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) = schicke(app(), &mark_done(&erste_id())).await;
        assert_eq!(status, StatusCode::OK);
        let ereignisse = lese_ereignisse(antwort.into_body(), 2).await;
        assert_eq!(ereignisse[1]["revision"], 1, "{ereignisse:?}");
    }

    // ---------- Agent-Takt ----------

    #[tokio::test]
    async fn takt_laesst_den_agenten_arbeiten_als_agent() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        // Ohne laufenden Agenten ändert der Takt nichts.
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
        assert_eq!(state.inner.live.revision(), 0);

        let ws = cockpit_core::demo::workspace();
        let ticket = ws
            .items
            .iter()
            .find(|i| i.key.starts_with("BAU-"))
            .expect("Ticket in den Beispieldaten")
            .key
            .clone();
        let start = serde_json::to_string(&ActionRequest {
            action: cockpit_core::Action::StartAgent {
                ticket: ticket.clone(),
                options: cockpit_core::AgentOptions {
                    plan_first: false,
                    ..Default::default()
                },
            },
        })
        .unwrap();
        let (status, body) = schicke(app(), &start).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let vorher = state.inner.live.revision();

        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
        assert_eq!(state.inner.live.revision(), vorher + 1);
        let (_, body) = hole(app(), "/api/workspace").await;
        let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
        assert_eq!(ws.log.last().unwrap().actor, Actor::Agent);
    }

    #[tokio::test]
    async fn takt_verknuepft_pull_requests_und_fuehrt_den_status_nach() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        // Der Branch von #85 nennt BAU-150: Der Takt verknüpft ihn (als System).
        let mut ws = cockpit_core::demo::workspace();
        ws.reviews
            .iter_mut()
            .find(|r| r.number == 85)
            .unwrap()
            .branch = "feature/bau-150-sheet".into();
        state.inner.live.setze_stand_fuer_test(ws);
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
        let (_, body) = hole(app(), "/api/workspace").await;
        let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
        assert_eq!(
            ws.review(85).unwrap().ticket_key.as_deref(),
            Some("BAU-150")
        );
        assert!(ws.log.iter().all(|e| e.actor == Actor::System));
        assert!(ws.log.len() >= 2, "Verknüpfung und Status: {:?}", ws.log);
        // Beim zweiten Takt gibt es nichts mehr zu tun.
        let vorher = state.inner.live.revision();
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
        assert_eq!(state.inner.live.revision(), vorher);
    }

    #[tokio::test]
    async fn takt_treibt_folgeauftraege_bis_zur_diagnose() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        let start = serde_json::to_string(&ActionRequest {
            action: cockpit_core::Action::InvestigateCheck {
                review: 85,
                check: "Lint".into(),
            },
        })
        .unwrap();
        let (status, body) = schicke(app(), &start).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        for _ in 0..3 {
            let s = state.clone();
            tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
        }
        let (_, body) = hole(app(), "/api/workspace").await;
        let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
        let lauf = ws.run("check-85-lint").unwrap();
        assert!(lauf.is_diagnosed(), "wartet auf deine Entscheidung");
        assert!(lauf.diagnosis.is_some());
        // Danach ruht der Takt, bis du den Fix übernimmst.
        let vorher = state.inner.live.revision();
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
        assert_eq!(state.inner.live.revision(), vorher);
        // Auch jetzt lässt sich nichts mergen: #85 hat rote Checks.
        let merge = serde_json::to_string(&ActionRequest {
            action: cockpit_core::Action::MergePr {
                review: 85,
                method: cockpit_core::MergeMethod::Squash,
            },
        })
        .unwrap();
        let (status, _) = schicke(app(), &merge).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn takt_holt_zurueckgestellte_eintraege_zurueck() {
        let state = zustand(Arc::new(MemoryStore::new()));
        let app = || router(state.clone(), PathBuf::from("/x"));
        let id = erste_id();
        let bis = jetzt_ms() + 60_000;
        let snooze = serde_json::to_string(&ActionRequest {
            action: cockpit_core::Action::Snooze {
                item: id.clone(),
                until_ms: Some(bis),
            },
        })
        .unwrap();
        let (status, body) = schicke(app(), &snooze).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let vorher = state.inner.live.revision();

        // Vor Ablauf tut der Takt nichts …
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.inner.live.takt(&s.inner.db, bis - 1, true))
            .await
            .unwrap();
        assert_eq!(state.inner.live.revision(), vorher);
        // … danach ist der Eintrag zurück, protokolliert als System.
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.inner.live.takt(&s.inner.db, bis + 1, true))
            .await
            .unwrap();
        assert_eq!(state.inner.live.revision(), vorher + 1);
        let (_, body) = hole(app(), "/api/workspace").await;
        let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
        assert!(ws.snoozed.is_empty());
        assert_eq!(ws.log.last().unwrap().actor, Actor::System);
    }

    // ---------- Persistenz ----------

    #[tokio::test]
    async fn stand_ueberlebt_den_neustart() {
        let tmp = tempfile::tempdir().unwrap();
        let datei = tmp.path().join("cockpit.db");
        let id = erste_id();

        let nach_aktion = {
            let state = AppState::new(db::open(&datei).unwrap(), Arc::new(MemoryStore::new()));
            let app = router(state.clone(), PathBuf::from("/x"));
            let (status, _) = schicke(app.clone(), &mark_done(&id)).await;
            assert_eq!(status, StatusCode::OK);
            hole(app, "/api/workspace").await.1
        };

        // Neuer Prozess: dieselbe Datei, frischer Zustand.
        let state = AppState::new(db::open(&datei).unwrap(), Arc::new(MemoryStore::new()));
        let app = router(state, PathBuf::from("/x"));
        let (_, nach_neustart) = hole(app.clone(), "/api/workspace").await;
        assert_eq!(nach_neustart, nach_aktion);
        let (ws, revision) = cockpit_core::persist::load_with_revision(&nach_neustart).unwrap();
        assert_eq!(revision, 1);
        assert!(ws.item(&id).unwrap().read);
        // Und es geht dort weiter, wo es aufgehört hat.
        let (_, body) = schicke(
            app,
            &format!(r#"{{"action":{{"SetStatus":{{"item":"{id}","status":"Review"}}}}}}"#),
        )
        .await;
        assert_eq!(json(&body)["revision"], 2, "{body}");
    }

    #[tokio::test]
    async fn unlesbarer_gespeicherter_stand_faellt_auf_demo_zurueck() {
        let conn = db::open_im_speicher().unwrap();
        db::workspace_speichern(&conn, 7, "{kaputt").unwrap();
        let state = AppState::new(conn, Arc::new(MemoryStore::new()));
        let app = router(state, PathBuf::from("/x"));
        let (_, body) = hole(app, "/api/workspace").await;
        let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
        assert_eq!(ws, cockpit_core::demo::workspace());
        assert_eq!(revision, 0);
    }

    // ---------- Seed, LiteLLM, Container ----------

    fn zustand_mit(conn: Connection, e: Einstellungen, secrets: Arc<MemoryStore>) -> AppState {
        AppState::neu(conn, secrets, 8787, Quellen::default(), e)
    }

    fn leer() -> Einstellungen {
        Einstellungen {
            suchpfad: Some(std::ffi::OsString::new()),
            ..Einstellungen::default()
        }
    }

    async fn stand_von(state: AppState) -> (cockpit_core::Workspace, u64) {
        let app = router(state, PathBuf::from("/x"));
        let (status, body) = hole(app, "/api/workspace").await;
        assert_eq!(status, StatusCode::OK);
        cockpit_core::persist::load_with_revision(&body).unwrap()
    }

    #[tokio::test]
    async fn ohne_seed_startet_der_workspace_leer() {
        let conn = db::open_im_speicher().unwrap();
        let (ws, revision) =
            stand_von(zustand_mit(conn, leer(), Arc::new(MemoryStore::new()))).await;
        assert_eq!(ws, cockpit_core::Workspace::default());
        assert!(ws.items.is_empty() && ws.reviews.is_empty() && ws.runs.is_empty());
        assert_eq!(revision, 0);
    }

    #[tokio::test]
    async fn seed_demo_laedt_die_beispieldaten() {
        let e = Einstellungen {
            seed: Seed::Demo,
            ..leer()
        };
        let conn = db::open_im_speicher().unwrap();
        let (ws, _) = stand_von(zustand_mit(conn, e, Arc::new(MemoryStore::new()))).await;
        assert_eq!(ws, cockpit_core::demo::workspace());
    }

    #[tokio::test]
    async fn ein_gespeicherter_stand_geht_dem_seed_vor() {
        let mut gespeichert = cockpit_core::demo::workspace();
        let id = gespeichert.items[0].id.clone();
        gespeichert.mark_done(&id);
        let text = cockpit_core::persist::save_with_revision(&gespeichert, 5);
        for seed in [Seed::Leer, Seed::Demo] {
            let conn = db::open_im_speicher().unwrap();
            db::workspace_speichern(&conn, 5, &text).unwrap();
            let e = Einstellungen { seed, ..leer() };
            let (ws, revision) =
                stand_von(zustand_mit(conn, e, Arc::new(MemoryStore::new()))).await;
            assert_eq!(ws, gespeichert, "{seed:?}");
            assert_eq!(revision, 5);
        }
    }

    #[tokio::test]
    async fn unlesbarer_stand_faellt_auf_den_seed_zurueck() {
        let conn = db::open_im_speicher().unwrap();
        db::workspace_speichern(&conn, 7, "{kaputt").unwrap();
        let (ws, revision) =
            stand_von(zustand_mit(conn, leer(), Arc::new(MemoryStore::new()))).await;
        assert_eq!(ws, cockpit_core::Workspace::default());
        assert_eq!(revision, 0);
    }

    #[tokio::test]
    async fn aktionen_auf_dem_leeren_workspace_lehnen_unbekannte_eintraege_ab() {
        let conn = db::open_im_speicher().unwrap();
        let app = router(
            zustand_mit(conn, leer(), Arc::new(MemoryStore::new())),
            PathBuf::from("/x"),
        );
        let (status, body) = schicke(app, &mark_done("gibt-es-nicht")).await;
        assert!(status.is_client_error(), "{status} {body}");
    }

    #[tokio::test]
    async fn health_meldet_litellm_ohne_den_key() {
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .set(secrets::LITELLM, &Secret::new("sk-geheim-123456"))
            .unwrap();
        let e = Einstellungen {
            litellm: Some(LitellmKonfig {
                url: "http://localhost:4000".into(),
                modell: Some("claude-sonnet-5-5".into()),
            }),
            ..leer()
        };
        let conn = db::open_im_speicher().unwrap();
        let app = router(zustand_mit(conn, e, secrets), PathBuf::from("/x"));
        let (_, body) = hole(app, "/api/health").await;
        assert!(!body.contains("geheim"), "{body}");
        let j = json(&body);
        assert_eq!(j["litellm"]["konfiguriert"], true);
        assert_eq!(j["litellm"]["url"], "http://localhost:4000");
        assert_eq!(j["litellm"]["modell"], "claude-sonnet-5-5");
        assert_eq!(j["litellm"]["key"], true);

        // Ohne Einstellung: nicht konfiguriert, kein Key.
        let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
        let (_, body) = hole(app, "/api/health").await;
        let j = json(&body);
        assert_eq!(j["litellm"]["konfiguriert"], false);
        assert_eq!(j["litellm"]["key"], false);
        assert_eq!(j["container"]["quelle"], "keine");
        assert_eq!(j["container"]["laufzeit"], serde_json::Value::Null);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn health_meldet_die_container_laufzeit() {
        use std::os::unix::fs::PermissionsExt;
        let ordner = tempfile::tempdir().unwrap();
        let datei = ordner.path().join("podman");
        std::fs::write(&datei, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&datei, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = Einstellungen {
            suchpfad: Some(ordner.path().as_os_str().to_owned()),
            ..Einstellungen::default()
        };
        let conn = db::open_im_speicher().unwrap();
        let app = router(
            zustand_mit(conn, e, Arc::new(MemoryStore::new())),
            PathBuf::from("/x"),
        );
        let (_, body) = hole(app, "/api/health").await;
        let j = json(&body);
        assert_eq!(j["container"]["laufzeit"], "podman");
        assert_eq!(j["container"]["engine_laeuft"], true);
        assert_eq!(j["container"]["quelle"], "erkannt");
    }

    fn mit_litellm(url: &str, secrets: Arc<MemoryStore>) -> Router {
        let e = Einstellungen {
            litellm: Some(LitellmKonfig {
                url: url.into(),
                modell: Some("claude-sonnet-5-5".into()),
            }),
            ..leer()
        };
        let conn = db::open_im_speicher().unwrap();
        router(zustand_mit(conn, e, secrets), PathBuf::from("/x"))
    }

    #[tokio::test]
    async fn litellm_pruefen_meldet_erreichbar_key_und_modell() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let proxy = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer sk-geheim-123456"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "claude-sonnet-5-5"}]})),
            )
            .expect(1)
            .mount(&proxy)
            .await;
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .set(secrets::LITELLM, &Secret::new("sk-geheim-123456"))
            .unwrap();
        let (status, body) = hole(mit_litellm(&proxy.uri(), secrets), "/api/litellm/pruefen").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("geheim"), "{body}");
        let j = json(&body);
        assert_eq!(j["konfiguriert"], true);
        assert_eq!(j["erreichbar"], true);
        assert_eq!(j["key_ok"], true);
        assert_eq!(j["modell_vorhanden"], true);
    }

    #[tokio::test]
    async fn litellm_pruefen_ohne_key_ruft_den_proxy_nicht_auf() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let proxy = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&proxy)
            .await;
        let (_, body) = hole(
            mit_litellm(&proxy.uri(), Arc::new(MemoryStore::new())),
            "/api/litellm/pruefen",
        )
        .await;
        let j = json(&body);
        assert_eq!(j["konfiguriert"], true);
        assert_eq!(j["key_ok"], false);
        assert!(j["hinweis"].as_str().unwrap().contains("litellm"));
    }

    #[tokio::test]
    async fn litellm_pruefen_ohne_einstellung_und_von_fremden_hosts() {
        let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
        let (status, body) = hole(app.clone(), "/api/litellm/pruefen").await;
        assert_eq!(status, StatusCode::OK);
        let j = json(&body);
        assert_eq!(j["konfiguriert"], false);
        assert!(j["hinweis"].as_str().unwrap().contains("[litellm]"));
        let req = Request::builder()
            .uri("/api/litellm/pruefen")
            .header("host", "boese.example")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(sende(app, req).await.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn health_meldet_den_agenten_nie_geheimnisse() {
        // Simuliert (Tests starten mit dem Seed demo): immer bereit.
        let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
        let (_, body) = hole(app, "/api/health").await;
        let j = json(&body);
        assert_eq!(j["agent"]["modus"], "simuliert");
        assert_eq!(j["agent"]["bereit"], true);
        // Echt, aber nichts eingerichtet: sagt in Klartext, was fehlt.
        let conn = db::open_im_speicher().unwrap();
        let e = Einstellungen {
            seed: Seed::Leer,
            ..leer()
        };
        let app = router(
            zustand_mit(conn, e, Arc::new(MemoryStore::new())),
            PathBuf::from("/x"),
        );
        let (_, body) = hole(app, "/api/health").await;
        let j = json(&body);
        assert_eq!(j["agent"]["modus"], "echt");
        assert_eq!(j["agent"]["bereit"], false);
        assert_eq!(j["agent"]["image_name"], "localhost/cockpit-agent:1");
        let fehlt = j["agent"]["fehlt"].as_array().unwrap();
        assert!(fehlt.len() >= 3);
        assert!(
            fehlt
                .iter()
                .all(|f| f.as_str().unwrap().contains("Einrichten"))
        );
    }

    #[tokio::test]
    async fn image_bauen_ohne_laufzeit_ist_422_mit_klartext() {
        let conn = db::open_im_speicher().unwrap();
        let e = Einstellungen {
            seed: Seed::Leer,
            ..leer()
        };
        let app = router(
            zustand_mit(conn, e, Arc::new(MemoryStore::new())),
            PathBuf::from("/x"),
        );
        let (status, body) = hole(app.clone(), "/api/agent/image").await;
        assert_eq!(status, StatusCode::OK);
        let j = json(&body);
        assert_eq!(j["laeuft"], false);
        assert_eq!(j["vorhanden"], false);
        let req = anfrage("POST", "/api/agent/image")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let (status, body) = sende(app, req).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            json(&body)["fehler"]
                .as_str()
                .unwrap()
                .contains("Rancher Desktop"),
            "{body}"
        );
    }
}
