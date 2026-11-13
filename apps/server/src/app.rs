//! The HTTP part: routes under `/api`, behind them the built web app.

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

/// This often the simulated agent takes a step (as in the demo mode of the app).
pub const AGENT_TICK: Duration = Duration::from_millis(2200);
/// This often a sign of life comes on the event connection.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Settings from `config.toml` and environment that do not concern Jira or GitHub.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub seed: Seed,
    /// `[agent] provider` and `[agent.endpoint]`.
    pub agent: AgentChoice,
    /// Replacement for `https://api.anthropic.com` so that tests never reach the real API.
    pub anthropic_api: Option<String>,
    /// `[agent] container`.
    pub container: Option<String>,
    /// `COCKPIT_CONTAINER` (goes before the file).
    pub container_env: Option<String>,
    /// Where to search for `docker` and `podman`; `None` = `PATH` of the process (tests set it).
    pub search_path: Option<std::ffi::OsString>,
    /// `COCKPIT_DEV_PORT`: the port of `trunk serve`, whose requests (host, origin) also apply.
    pub dev_port: Option<u16>,
    /// The `config.toml` into which the setup assistant writes.
    pub config_path: Option<PathBuf>,
    /// `[agent] repo`: the default repo in which the agent works.
    pub agent_repo: Option<String>,
    /// `[[agent.repos]]`: the repos the agent may work in.
    pub agent_repos: Vec<cockpit_core::AgentRepo>,
    /// Folder with Containerfile and runner of the agent.
    pub agent_dir: Option<PathBuf>,
    /// Base address for cloning instead of GitHub (tests only).
    pub git_base: Option<String>,
    /// Here the agent puts clones and jobs (next to the database).
    pub data_dir: Option<PathBuf>,
    /// Fixed upper bound in seconds for a run instead of the limits of the workspace (tests only).
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
            agent_repos: config.agent_repos.clone(),
            agent_dir: Some(agent_dir_find(config)),
            git_base: config.git_base.clone(),
            data_dir: config.db.parent().map(PathBuf::from),
            run_seconds: None,
        }
    }
}

/// The folder `agent` with Containerfile and runner: `[agent] dir` or `COCKPIT_AGENT_DIR`, otherwise
/// next to the built app (`<dist>/../../agent`, so it lies in the repo and in the release archive),
/// otherwise `./apps/agent`.
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

/// What the configuration says about the repos of the agent.
#[derive(Debug, Clone, Default)]
pub(crate) struct RepoConfig {
    pub repos: Vec<cockpit_core::AgentRepo>,
    pub default: Option<String>,
}

/// Shared state of the handlers. Cloning is cheap.
#[derive(Clone)]
pub struct AppState {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) db: Mutex<Connection>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) live: Live,
    /// The port on which the server listens – for checking `Host` and `Origin`.
    pub(crate) port: u16,
    /// Open OAuth sign-ins (`state` → PKCE verifier), only in memory.
    pub(crate) sign_ins: Mutex<cockpit_integrations::oauth::SignIns>,
    /// Only one refresh of the OAuth tokens at a time (refresh tokens rotate).
    pub(crate) refresh: tokio::sync::Mutex<()>,
    /// Jira and GitHub as configured (whether they run still depends on the key).
    /// Is replaced when saving in the setup assistant (without restart).
    pub(crate) sources: RwLock<Arc<Sources>>,
    /// Provider and endpoint of the agent, also replaceable.
    pub(crate) agent_choice: RwLock<AgentChoice>,
    /// The repos of the agent (`[[agent.repos]]`, `[agent] repo`), also replaceable.
    pub(crate) agent_repo_config: RwLock<RepoConfig>,
    /// Reading, changing and writing `config.toml` go one after another.
    pub(crate) config_lock: Mutex<()>,
    /// Writes to the outside and taking over a sync go one after another.
    pub(crate) lock: tokio::sync::Mutex<()>,
    /// Counts successful writes: a fetch that began in the meantime is outdated.
    pub(crate) written: AtomicU64,
    /// Only one sync at a time.
    pub(crate) sync: tokio::sync::Mutex<()>,
    pub(crate) settings: Settings,
    /// Result of the search for the container runtime, remembered briefly.
    pub(crate) container: Cache,
    /// Runs of the real agent (container) and the build of the image.
    pub(crate) agent: crate::agent_ops::Runs,
}

impl AppState {
    /// Like [`AppState::with_port`], with the default port (for tests).
    #[cfg(test)]
    pub fn new(db: Connection, secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_port(db, secrets, crate::config::DEFAULT_PORT)
    }

    /// Loads the stored workspace in the process (otherwise the sample data).
    #[cfg(test)]
    pub fn with_port(db: Connection, secrets: Arc<dyn SecretStore>, port: u16) -> Self {
        Self::with_sources(db, secrets, port, Sources::default())
    }

    /// Like [`AppState::with_port`], with Jira and GitHub for the sync. The tests start with
    /// the sample data, the server itself with an empty workspace ([`AppState::create`]).
    #[cfg(test)]
    pub fn with_sources(
        db: Connection,
        secrets: Arc<dyn SecretStore>,
        port: u16,
        sources: Sources,
    ) -> Self {
        let settings = Settings {
            seed: Seed::Demo,
            // Tests search nowhere for container programs: same result on every machine.
            search_path: Some(std::ffi::OsString::new()),
            ..Settings::default()
        };
        Self::create(db, secrets, port, sources, settings)
    }

    /// The state of the server. A stored workspace is loaded, otherwise the seed applies.
    pub fn create(
        db: Connection,
        secrets: Arc<dyn SecretStore>,
        port: u16,
        sources: Sources,
        settings: Settings,
    ) -> Self {
        let live = Live::load(&db, settings.seed);
        // An old state gets its repos: from the sync links and entries, otherwise from the only
        // configured repo.
        let only_repo = {
            let github = sources
                .github
                .as_ref()
                .map(|g| g.repos.clone())
                .unwrap_or_default();
            let repos = crate::agent_run::repos_effective(
                &settings.agent_repos,
                settings.agent_repo.as_deref(),
                &github,
            );
            (repos.repos.len() == 1).then(|| repos.repos[0].repo.clone())
        };
        live.fill_repos(&db, only_repo.as_deref());
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
                agent_repo_config: RwLock::new(RepoConfig {
                    repos: settings.agent_repos.clone(),
                    default: settings.agent_repo.clone(),
                }),
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

    /// Jira and GitHub as last configured.
    pub(crate) fn sources(&self) -> Arc<Sources> {
        match self.inner.sources.read() {
            Ok(q) => q.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Provider and endpoint of the agent as last configured.
    pub(crate) fn agent_choice(&self) -> AgentChoice {
        match self.inner.agent_choice.read() {
            Ok(l) => l.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Takes over a newly written configuration without restart: sources, OAuth apps and
    /// agent endpoint. Port, database, sync tick and container choice stay as they are.
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
        let repos = RepoConfig {
            repos: c.agent_repos.clone(),
            default: c.agent_repo.clone(),
        };
        match self.inner.agent_repo_config.write() {
            Ok(mut l) => *l = repos,
            Err(v) => *v.into_inner() = repos,
        }
    }

    /// A tick of the tasks in the background. The simulated agent only joins in if the server
    /// runs with the seed `demo`; otherwise the real agent works in containers. Blocking.
    pub fn tick(&self) {
        self.inner
            .live
            .tick(&self.inner.db, now_ms(), !self.agent_real());
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Starts the background work of the agent in the server: in mode "real" the watcher for the
/// containers, with the seed `demo` the tick of the simulated agent (instead of in the browser).
pub fn agent_tick_start(state: AppState) {
    if state.agent_real() {
        crate::agent_ops::watcher_start(state);
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(AGENT_TICK);
        ticker.tick().await; // the first tick comes immediately
        loop {
            ticker.tick().await;
            let state = state.clone();
            let _ = tokio::task::spawn_blocking(move || state.tick()).await;
        }
    });
}

/// Whether a connection is set up: it is in the database **and** its key
/// is in the key store. The key itself is never read or printed.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Connections {
    pub github: bool,
    pub jira: bool,
}

/// Kind and state of the sign-in per service – for "Mit … anmelden" and "Trennen".
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SignIn {
    pub github: ServiceStatus,
    pub jira: ServiceStatus,
}

/// State of the sync per source: `off` (not set up or without key), `waiting`
/// (no fetch yet), `ok`, `unauthorized`, `no_permission`, `quota`,
/// `offline` or `error`.
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
    /// Provider, address, model and whether a key is present – never the key.
    pub endpoint: EndpointStatus,
    pub container: ContainerStatus,
    /// Whether the real agent can start and what is missing.
    pub agent: AgentStatus,
    /// Where the `.env` file with the keys lives and what to warn about – never a value.
    pub env_file: Option<secrets::EnvFileStatus>,
}

impl AppState {
    /// Blocking (database, .env file) – do not call directly in an async handler.
    fn health(&self) -> Health {
        let names = match self.inner.db.lock() {
            Ok(conn) => db::connections(&conn).unwrap_or_else(|e| {
                tracing::warn!("Could not read connections: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        // A token (database entry **and** key) or the sign-in via OAuth.
        let set_up = |name: &str| {
            names.iter().any(|n| n == name)
                && self.inner.secrets.has(name).unwrap_or_else(|e| {
                    tracing::warn!("Could not check key \"{name}\": {e}");
                    false
                })
        };
        let github = self.service_status(Service::GitHub, set_up(secrets::GITHUB));
        // Jira via token needs address and email from the configuration (entry in the database).
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
            env_file: self.inner.secrets.env_file().map(secrets::env_file_status),
        }
    }
}

async fn health(State(state): State<AppState>) -> Result<Json<Health>, StatusCode> {
    tokio::task::spawn_blocking(move || state.health())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// The key of the endpoint from the .env file: `agent-key`, alternatively the older `litellm`.
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
    /// Where the calls to the endpoint go: the entered address, otherwise directly Anthropic.
    pub(crate) fn endpoint_target(&self) -> cockpit_integrations::endpoint::Target {
        let choice = self.agent_choice();
        cockpit_integrations::endpoint::Target::from(
            choice.endpoint.as_ref().and_then(|e| e.url.as_deref()),
            self.inner.settings.anthropic_api.as_deref(),
        )
    }

    /// The key of the endpoint. `Err` is already a finished response.
    async fn endpoint_key(&self) -> Result<Option<cockpit_integrations::Secret>, Box<Response>> {
        let secrets = self.inner.secrets.clone();
        match tokio::task::spawn_blocking(move || agent_key_read(secrets.as_ref())).await {
            Ok(Ok(k)) => Ok(k),
            Ok(Err(e)) => {
                tracing::warn!("Agent endpoint key not readable: {e}");
                Err(Box::new(error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Die .env-Datei im Projektordner ist nicht lesbar",
                )))
            }
            Err(_) => Err(Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response())),
        }
    }
}

/// `GET /api/agent/check`: only calls `GET <url>/v1/models` with the
/// key and reports whether the endpoint is reachable, accepts the key and offers the model.
/// Never the key.
async fn endpoint_check(State(state): State<AppState>) -> Response {
    let choice = state.agent_choice();
    let Some(e) = choice.endpoint.clone() else {
        // Without an entry there is nothing to check (not even "direct": first the person chooses a model).
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

/// `GET /api/agent/models`: the model IDs of the endpoint (IDs only, sorted) for the drop-down.
/// The key comes from the .env file and never leaves the backend.
async fn agent_models(State(state): State<AppState>) -> Response {
    let key = match state.endpoint_key().await {
        Ok(k) => k,
        Err(answer) => return *answer,
    };
    Json(cockpit_integrations::endpoint::models_load(&state.endpoint_target(), key.as_ref()).await)
        .into_response()
}

/// `GET /api/agent/image`: state of "Image bauen" (running, present, last lines, error).
async fn agent_image_read(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_state()).await {
        Ok(b) => Json(b).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/agent/image`: builds the agent image in the background (`docker build` or `podman build`).
/// The progress is in `GET /api/agent/image`.
async fn agent_image_build(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_build()).await {
        Ok(Ok(b)) => Json(b).into_response(),
        Ok(Err(m)) => error(StatusCode::UNPROCESSABLE_ENTITY, m),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/agent/check/container`: does a
/// container reach the endpoint (check mode of the image)?
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

/// `GET /api/workspace`: the current state, shape of `cockpit_core::persist` plus `revision`.
async fn workspace(State(state): State<AppState>) -> Response {
    (
        [(CONTENT_TYPE, "application/json")],
        state.inner.live.snapshot(),
    )
        .into_response()
}

/// `POST /api/actions`: applies the action as "Du" with the time of the server. If it acts
/// to the outside (Jira, GitHub), the call to the service goes first; if it fails, nothing
/// changes. We read the body ourselves so that an unknown action gets a German message.
async fn action(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<ActionRequest>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "Die Anfrage ist keine bekannte Aktion",
        );
    };
    // Another Jira project: sync right away so that its tickets arrive. The result
    // reaches the app through the event stream.
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
                        tracing::warn!("Sync after project change: {e:?}");
                    }
                });
            }
            Json(answer).into_response()
        }
        Err(e) => action_error(e),
    }
}

/// `POST /api/sync`: "jetzt abgleichen".
async fn sync(State(state): State<AppState>) -> Response {
    match state.sync().await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

/// `GET /api/jira/projects`: the projects for the selection in the inbox.
async fn jira_projects(State(state): State<AppState>) -> Response {
    match state.jira_projects().await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

fn action_error(e: ActionFailure) -> Response {
    match e {
        // All rejections by the rules are 422 – without listing individual errors.
        ActionFailure::Rule(e) => error(StatusCode::UNPROCESSABLE_ENTITY, e.message()),
        ActionFailure::Plan(m) => error(StatusCode::UNPROCESSABLE_ENTITY, m),
        // The service has rejected or is not reachable.
        ActionFailure::Service(m) => error(StatusCode::BAD_GATEWAY, m),
        ActionFailure::Save(m) => {
            tracing::error!("Workspace not saved: {m}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Stand konnte nicht gespeichert werden",
            )
        }
        ActionFailure::Intern => error(StatusCode::INTERNAL_SERVER_ERROR, "Interner Fehler"),
    }
}

/// `GET /api/events`: server-sent events. First the current revision, then one event
/// per applied action. Whoever reads too slowly gets the current revision and reloads.
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

/// Builds the router. `dist` is the folder of the built web app.
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
        .route("/api/github/repos/check", post(crate::setup::repos_check))
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
        // Applies only to `/api`, not to the files of the app.
        .layer(axum::middleware::from_fn_with_state(
            access,
            access::protect,
        ))
        .with_state(state)
        .fallback_service(ServeDir::new(dist))
}

#[cfg(test)]
mod tests;
