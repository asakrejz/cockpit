//! The shared state of the server and its settings.

use super::*;

/// Settings from `config.toml` and environment that do not concern Jira or GitHub.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub seed: Seed,
    /// `[agent] provider` and `[agent.endpoint]`.
    pub agent: AgentChoice,
    /// Replacement for `https://api.anthropic.com` (and `https://api.openai.com` for Codex) so that tests never reach
    /// the real API.
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
    /// The folder `config/` (Git and permission checks in `/api/health`).
    pub config_dir: Option<PathBuf>,
    /// Places of old files, reported while they still exist.
    pub legacy_files: Vec<PathBuf>,
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
    /// `[server] webhooks`, `public_url` and `[sync] webhook_fallback_interval_s`.
    pub webhooks: crate::webhooks::WebhookSettings,
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
            config_dir: config.config_dir.clone(),
            legacy_files: config.legacy_files.clone(),
            agent_repo: config.agent_repo.clone(),
            agent_repos: config.agent_repos.clone(),
            agent_dir: Some(agent_dir_find(config)),
            git_base: config.git_base.clone(),
            data_dir: config.db.parent().map(PathBuf::from),
            run_seconds: None,
            webhooks: crate::webhooks::WebhookSettings {
                enabled: config.webhooks,
                public_url: config.public_url.clone(),
                fallback_s: config.webhook_fallback_s(),
                ..Default::default()
            },
        }
    }
}

/// The folder `agent` with Containerfile and runner: `[agent] dir` or `COCKPIT_AGENT_DIR`, otherwise
/// next to the built app (`<dist>/../../agent`, so it lies in the repo and in the release archive),
/// otherwise `./apps/agent`.
pub(super) fn agent_dir_find(config: &Config) -> PathBuf {
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
    /// Optional webhooks: switch, debounce, rate limit, last received (memory only).
    pub(crate) webhooks: crate::webhooks::Hub,
    /// Attachments read from Jira, kept briefly (see `attachments`).
    pub(crate) attachments: crate::attachments::Cache,
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
                container: Cache::default(),
                agent: crate::agent_ops::Runs::default(),
                webhooks: crate::webhooks::Hub::new(settings.webhooks.clone()),
                attachments: Default::default(),
                settings,
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
            pr_limits: c.github_pr_limits,
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
        self.inner.webhooks.apply(c.webhooks, c.public_url.clone());
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
