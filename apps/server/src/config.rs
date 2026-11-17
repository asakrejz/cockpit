//! Configuration: defaults, above them the file, above that environment variables.
//!
//! The file (`config/config.toml` in the project folder, created at the first start; see
//! `location`) contains **no keys** – only addresses and accounts. The database is
//! `config/cockpit.db` next to it. `COCKPIT_CONFIG` and `COCKPIT_DB` override the places.
//!
//! ```toml
//! port = 8787
//! dist = "/path/to/apps/cockpit/dist"
//! db = "/path/to/cockpit.db"
//!
//! interval_s = 120                     # sync every 2 minutes (at least 30); older: `intervall_s`
//!
//! [github]
//! api_url = "https://api.github.com"   # optional
//! repos = ["firma/app", "firma/web"]   # repos for the sync (owner/name)
//!
//! [jira]
//! site = "https://firma.atlassian.net"
//! email = "ich@firma.example"
//! projects = ["BAU", "WEB"]            # optional: only these projects
//! jql = "project = BAU AND ..."        # optional: own query instead of the projects
//! ```
//!
//! Sign-in via OAuth (instead of an API token), only client IDs – the secrets are in the
//! .env file (`cockpit-server token set github-oauth|atlassian-oauth`):
//!
//! ```toml
//! [github_oauth]
//! client_id = "Iv23li…"                # GitHub App
//!
//! [atlassian]
//! client_id = "7dNimh…"                # OAuth-2.0-3LO-App
//! site = "firma"                       # optional: name, address or id, if several are approved
//! ```
//!
//! With `[atlassian]` `[jira]` needs neither `site` nor `email` (only `projects`/`jql`).
//!
//! Agent provider and endpoint (only address and model name – the key is in the .env file,
//! `cockpit-server token set agent-key`), container runtime and seed:
//!
//! ```toml
//! seed = "demo"                        # sample data, only if no state has been stored yet
//!
//! [agent]
//! provider = "claude-agent-sdk"        # or "claude-code" or "codex"
//! container = "podman"                 # or "docker"; without a value the backend detects the runtime
//! repo = "firma/app"                   # default repo of the agent (otherwise the only one from [github] repos)
//! dir = "/path/to/agent"               # folder with Containerfile and runner (otherwise ./apps/agent or next to apps/cockpit/dist)
//!
//! # Several repos: a Jira project can be routed to a repo. A ticket whose project is not named
//! # here goes to the default repo (`repo` above); the interface lets you change it before the start.
//! [[agent.repos]]
//! repo = "firma/web"
//! base_branch = "develop"              # optional, default "main"
//! projects = ["WEB", "SHOP"]           # Jira projects whose tickets go to this repo
//!
//! [[agent.repos]]
//! repo = "firma/app"                   # a repo without projects is only a choice
//!
//! [agent.endpoint]
//! url = "https://litellm.example"      # proxy or compatible endpoint; empty or omitted = directly at Anthropic
//! model = "claude-sonnet-5-5"          # name as the endpoint offers it
//! ```
//!
//! Optional webhooks (speed up the sync; off by default, see docs/setup-guide.md):
//!
//! ```toml
//! [server]
//! webhooks = true                          # the two routes /api/webhooks/{github,jira} answer
//! public_url = "https://cockpit.example"   # your tunnel, only shown in the assistant
//!
//! [sync]
//! webhook_fallback_interval_s = 600        # poll interval while webhooks arrive (default: 5x interval_s, at most 600)
//! ```
//!
//! Older files are still read: `[litellm]` (`url`, `modell`) and the key `litellm`
//! (equivalent to `[agent.endpoint]` and `agent-key`), `[agent] anbieter`, `[agent.endpunkt]`
//! with `modell`, and `intervall_s`; if both are present the new one applies.
//! The assistant writes the new format.
//!
//! A source only syncs if it is set up here **and** its key is in the
//! .env file (`cockpit-server token set <github|jira>`). Without a source the
//! workspace stays empty (or shows the sample data if the seed `demo` is chosen).
//!
//! Umgebungsvariablen: `COCKPIT_PORT`, `COCKPIT_DIST`, `COCKPIT_DB`, `COCKPIT_CONFIG`
//! (path of the configuration file), `COCKPIT_SEED` (`empty` or `demo`; `leer` still applies), `COCKPIT_CONTAINER`
//! (`docker` or `podman`), `COCKPIT_AGENT_DIR` (folder `agent`), `COCKPIT_GIT_BASE` (older `COCKPIT_GIT_BASIS`; base address for cloning, tests only),
//! `COCKPIT_DEV_PORT` (port of `trunk serve`, for development only), `COCKPIT_ENV_FILE` (path of the `.env` file with the keys).

use std::path::{Path, PathBuf};

use cockpit_core::api::Provider;
use cockpit_core::{AgentRepo, is_project_key};
use serde::Deserialize;

mod file;
use file::*;

pub mod files_check;
pub mod location;

mod sections;
pub use sections::*;

pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_GITHUB_API: &str = "https://api.github.com";
/// This often a sync happens if nothing is set.
pub const DEFAULT_INTERVAL_S: u64 = 120;
/// Shorter is not possible: the services limit the number of fetches.
pub const MIN_INTERVAL_S: u64 = 30;
/// Poll interval of a source while webhooks arrive: this multiple of the interval ...
pub const WEBHOOK_FALLBACK_FACTOR: u64 = 5;
/// ... but at most this long, unless set explicitly.
pub const WEBHOOK_FALLBACK_MAX_S: u64 = 600;
/// Upper bound of an explicit `webhook_fallback_interval_s`.
pub const WEBHOOK_FALLBACK_LIMIT_S: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    /// Folder of the built web app (`apps/cockpit/dist`).
    pub dist: PathBuf,
    pub db: PathBuf,
    pub github: Option<GitHubConfig>,
    pub jira: Option<JiraConfig>,
    pub github_oauth: Option<OAuthConfig>,
    pub atlassian: Option<AtlassianConfig>,
    /// Interval of the sync in seconds.
    pub interval_s: u64,
    /// What a workspace that has never been stored starts with.
    pub seed: Seed,
    /// `[agent] provider`: the tool of the agent.
    pub provider: Provider,
    /// `[agent.endpoint]` (or the older `[litellm]`).
    pub endpoint: Option<EndpointConfig>,
    /// Chosen container runtime (`docker` or `podman`) from the file; `COCKPIT_CONTAINER`
    /// is in [`Config::container_env`].
    pub container: Option<String>,
    /// Value of `COCKPIT_CONTAINER`, goes before the file.
    pub container_env: Option<String>,
    /// `[agent] repo`: the default repo in which the agent works (`owner/name`).
    pub agent_repo: Option<String>,
    /// `[[agent.repos]]`: the repos the agent may work in, with base branch and Jira projects.
    pub agent_repos: Vec<AgentRepo>,
    /// `[agent] dir` or `COCKPIT_AGENT_DIR`: the folder with Containerfile and runner.
    pub agent_dir: Option<PathBuf>,
    /// `COCKPIT_GIT_BASE`: base address for cloning instead of `https://github.com` (for tests).
    pub git_base: Option<String>,
    /// `COCKPIT_DEV_PORT`: port of `trunk serve` for development (environment only, never file).
    pub dev_port: Option<u16>,
    /// The file that was read from (and that the setup assistant writes to); `None`
    /// if there is no configuration directory.
    pub config_path: Option<PathBuf>,
    /// The folder `config/` in the project folder, if the default places are in use (checked by Git and permission rules).
    pub config_dir: Option<PathBuf>,
    /// Places of old files (before `config/`), reported while they still exist.
    pub legacy_files: Vec<PathBuf>,
    /// `[server] webhooks`: the webhook routes answer (default `false`: 404).
    pub webhooks: bool,
    /// `[server] public_url`: address of the tunnel, only shown in the assistant.
    pub public_url: Option<String>,
    /// `[sync] webhook_fallback_interval_s`; `None` = [`Config::webhook_fallback_s`] decides.
    pub webhook_fallback_interval_s: Option<u64>,
}

/// What the server starts with as long as no state has been stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Seed {
    /// An empty workspace (default).
    #[default]
    Empty,
    /// The sample data from `cockpit_core::demo`.
    Demo,
}

impl Seed {
    fn from_text(text: &str) -> Result<Self, ConfigError> {
        match text.trim().to_lowercase().as_str() {
            "" | "empty" | "leer" => Ok(Seed::Empty),
            "demo" => Ok(Seed::Demo),
            other => Err(ConfigError::Wert(format!(
                "seed „{other}“ ist unbekannt, erlaubt sind empty und demo"
            ))),
        }
    }
}

/// The poll interval while webhooks arrive: explicit value, otherwise 5x the interval, at most
/// 10 minutes – and never shorter than the interval itself.
pub fn fallback_s(interval_s: u64, explicit: Option<u64>) -> u64 {
    explicit.unwrap_or_else(|| {
        interval_s
            .saturating_mul(WEBHOOK_FALLBACK_FACTOR)
            .min(WEBHOOK_FALLBACK_MAX_S)
            .max(interval_s)
    })
}

fn default_db_path() -> PathBuf {
    location::default_db_path(&location::project_dir())
}

impl Config {
    /// Defaults only.
    pub fn defaults() -> Self {
        Self {
            port: DEFAULT_PORT,
            dist: PathBuf::from("apps/cockpit/dist"),
            db: default_db_path(),
            github: None,
            jira: None,
            github_oauth: None,
            atlassian: None,
            interval_s: DEFAULT_INTERVAL_S,
            seed: Seed::Empty,
            provider: Provider::default(),
            endpoint: None,
            container: None,
            container_env: None,
            agent_repo: None,
            agent_repos: Vec::new(),
            agent_dir: None,
            git_base: None,
            dev_port: None,
            config_path: None,
            config_dir: None,
            legacy_files: Vec::new(),
            webhooks: false,
            public_url: None,
            webhook_fallback_interval_s: None,
        }
    }

    /// Poll interval of a source while its webhooks arrive regularly.
    pub fn webhook_fallback_s(&self) -> u64 {
        fallback_s(self.interval_s, self.webhook_fallback_interval_s)
    }

    /// Defaults → file content → environment. `file` is the text of the TOML file (if
    /// present), `env` delivers values of environment variables.
    pub fn parse(
        file: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let d: File = match file {
            Some(text) => {
                toml::from_str(text).map_err(|e| ConfigError::Toml(e.message().to_string()))?
            }
            None => File::default(),
        };
        let mut c = Self::defaults();
        if let Some(p) = d.port {
            c.port = p;
        }
        if let Some(p) = d.dist {
            c.dist = p;
        }
        if let Some(p) = d.db {
            c.db = p;
        }
        if let Some(i) = d.interval_s {
            if i < MIN_INTERVAL_S {
                return Err(ConfigError::Wert(format!(
                    "interval_s = {i} ist zu kurz, mindestens {MIN_INTERVAL_S} Sekunden"
                )));
            }
            c.interval_s = i;
        }
        if let Some(g) = d.github {
            for r in &g.repos {
                if !repo_valid(r) {
                    return Err(ConfigError::Wert(format!(
                        "GitHub-Repo „{r}“ ist nicht von der Form owner/name"
                    )));
                }
            }
            c.github = Some(GitHubConfig {
                api_url: g.api_url.unwrap_or_else(|| DEFAULT_GITHUB_API.into()),
                repos: g.repos,
            });
        }
        let client_id = |wer: &str, id: String| {
            let id = id.trim().to_string();
            if id.is_empty() || id.chars().any(char::is_whitespace) {
                Err(ConfigError::Wert(format!(
                    "{wer}: client_id fehlt oder enthält Leerzeichen"
                )))
            } else {
                Ok(id)
            }
        };
        if let Some(o) = d.github_oauth {
            c.github_oauth = Some(OAuthConfig {
                client_id: client_id("[github_oauth]", o.client_id)?,
            });
        }
        if let Some(a) = d.atlassian {
            c.atlassian = Some(AtlassianConfig {
                client_id: client_id("[atlassian]", a.client_id)?,
                site: a
                    .site
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            });
        }
        // With Atlassian sign-in Jira is enough without own details.
        let jira = d.jira.or_else(|| {
            c.atlassian.as_ref().map(|_| FileJira {
                site: String::new(),
                email: String::new(),
                projects: Vec::new(),
                jql: None,
            })
        });
        if let Some(j) = jira {
            if c.atlassian.is_none() && (j.site.trim().is_empty() || j.email.trim().is_empty()) {
                return Err(ConfigError::Wert(
                    "[jira] braucht site und email – oder [atlassian] für die Anmeldung per OAuth"
                        .into(),
                ));
            }
            for p in &j.projects {
                if cockpit_integrations::jira::jql_for(std::slice::from_ref(p), None).is_err() {
                    return Err(ConfigError::Wert(format!(
                        "Jira-Projekt „{p}“ ist kein gültiger Schlüssel"
                    )));
                }
            }
            c.jira = Some(JiraConfig {
                site: j.site,
                email: j.email,
                projects: j.projects,
                jql: j.jql.filter(|q| !q.trim().is_empty()),
            });
        }

        if let Some(text) = d.seed {
            c.seed = Seed::from_text(&text)?;
        }
        // The older `[litellm]` applies as long as `[agent.endpoint]` is missing.
        let mut endpoint = d.litellm.map(|l| ("[litellm]", l));
        if let Some(a) = d.agent {
            if let Some(text) = a.provider {
                c.provider = Provider::from_id(&text).ok_or_else(|| {
                    ConfigError::Wert(format!(
                        "[agent] provider „{}“ ist unbekannt, erlaubt sind claude-agent-sdk, claude-code und codex",
                        text.trim()
                    ))
                })?;
            }
            if let Some(e) = a.endpoint {
                endpoint = Some(("[agent.endpoint]", e));
            }
            c.container = a.container.map(|t| runtime(&t)).transpose()?;
            c.agent_repo = a
                .repo
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty());
            if let Some(r) = &c.agent_repo
                && !crate::agent_git::repo_name_ok(r)
            {
                return Err(ConfigError::Wert(format!(
                    "[agent] repo „{r}“ muss die Form owner/name haben"
                )));
            }
            c.agent_repos = agent_repos_read(&a.repos)?;
            c.agent_dir = a.dir;
        }
        if let Some((wo, e)) = endpoint {
            let url = e.url.map(|u| u.trim().trim_end_matches('/').to_string());
            let url = match url.filter(|u| !u.is_empty()) {
                None => None,
                Some(u) => {
                    let valid = u
                        .strip_prefix("http://")
                        .or_else(|| u.strip_prefix("https://"))
                        .is_some_and(|rest| {
                            !rest.is_empty() && !rest.contains(char::is_whitespace)
                        });
                    if !valid {
                        return Err(ConfigError::Wert(format!(
                            "{wo} url muss mit http:// oder https:// beginnen, z. B. http://localhost:4000"
                        )));
                    }
                    Some(u)
                }
            };
            let model = e
                .model
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty());
            if url.is_some() || model.is_some() {
                c.endpoint = Some(EndpointConfig { url, model });
            }
        }
        if let Some(w) = d.server {
            c.webhooks = w.webhooks;
            c.public_url = Some(
                cockpit_core::setup::public_url_read(w.public_url.as_deref().unwrap_or(""))
                    .map_err(ConfigError::Wert)?,
            )
            .filter(|u| !u.is_empty());
        }
        if let Some(i) = d.sync.and_then(|s| s.webhook_fallback_interval_s) {
            if !(c.interval_s..=WEBHOOK_FALLBACK_LIMIT_S).contains(&i) {
                return Err(ConfigError::Wert(format!(
                    "[sync] webhook_fallback_interval_s = {i} muss zwischen interval_s ({}) und {WEBHOOK_FALLBACK_LIMIT_S} liegen",
                    c.interval_s
                )));
            }
            c.webhook_fallback_interval_s = Some(i);
        }
        if let Some(p) = env("COCKPIT_AGENT_DIR").filter(|s| !s.trim().is_empty()) {
            c.agent_dir = Some(PathBuf::from(p.trim()));
        }
        if let Some(p) = env("COCKPIT_GIT_BASE")
            .or_else(|| env("COCKPIT_GIT_BASIS"))
            .filter(|s| !s.trim().is_empty())
        {
            c.git_base = Some(p.trim().trim_end_matches('/').to_string());
        }

        if let Some(p) = env("COCKPIT_SEED").filter(|s| !s.trim().is_empty()) {
            c.seed = Seed::from_text(&p)?;
        }
        if let Some(p) = env("COCKPIT_CONTAINER").filter(|s| !s.trim().is_empty()) {
            c.container_env = Some(runtime(&p)?);
        }
        if let Some(p) = env("COCKPIT_DEV_PORT").filter(|s| !s.trim().is_empty()) {
            let nr: u16 = p.trim().parse().map_err(|_| {
                ConfigError::Wert(format!("COCKPIT_DEV_PORT={p} ist keine Portnummer"))
            })?;
            if nr == 0 {
                return Err(ConfigError::Wert(
                    "COCKPIT_DEV_PORT 0 ist nicht erlaubt".into(),
                ));
            }
            c.dev_port = Some(nr);
        }
        if let Some(p) = env("COCKPIT_PORT").filter(|s| !s.is_empty()) {
            c.port = p
                .trim()
                .parse()
                .map_err(|_| ConfigError::Wert(format!("COCKPIT_PORT={p} ist keine Portnummer")))?;
        }
        if let Some(p) = env("COCKPIT_DIST").filter(|s| !s.is_empty()) {
            c.dist = p.into();
        }
        if let Some(p) = env("COCKPIT_DB").filter(|s| !s.is_empty()) {
            c.db = p.into();
        }
        if c.port == 0 {
            return Err(ConfigError::Wert("Port 0 ist nicht erlaubt".into()));
        }
        Ok(c)
    }

    /// Loads the file (`COCKPIT_CONFIG` or default path; if it is missing the
    /// defaults apply) and puts the environment variables on top.
    pub fn load() -> Result<Self, ConfigError> {
        let env = |k: &str| std::env::var(k).ok();
        let project = location::project_dir();
        let notes =
            location::prepare(&project, &env, location::is_macos()).map_err(ConfigError::Wert)?;
        for n in notes {
            tracing::info!("{n}");
        }
        let (path, explicit) = match env("COCKPIT_CONFIG").filter(|s| !s.is_empty()) {
            Some(p) => (Some(PathBuf::from(p)), true),
            None => (Some(location::default_config_path(&project)), false),
        };
        let text = match path.as_deref().map(read_optional) {
            Some(Ok(t)) => t,
            Some(Err((p, e))) => return Err(ConfigError::Read(p, e)),
            None => None,
        };
        if explicit && text.is_none() {
            let p = path.unwrap_or_default();
            return Err(ConfigError::Read(p, std::io::ErrorKind::NotFound.into()));
        }
        let mut c = Self::parse(text.as_deref(), env)?;
        c.config_path = path;
        let overrides = env("COCKPIT_CONFIG").is_some_and(|v| !v.is_empty())
            && env("COCKPIT_DB").is_some_and(|v| !v.is_empty());
        c.config_dir = (!overrides).then(|| location::config_dir(&project));
        c.legacy_files = location::legacy_candidates(&env, location::is_macos());
        Ok(c)
    }
}

#[cfg(test)]
mod tests;
