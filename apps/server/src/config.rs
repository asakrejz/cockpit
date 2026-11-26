//! Configuration: defaults, above them the file, above that environment variables.
//!
//! The reference for every key and every environment variable is `docs/configuration.md`; the
//! commented example is `config/config.example.toml`. This module reads the file
//! (`config/config.toml` in the project folder, see `location`), rejects keys it does not know
//! (`file`) and applies the environment variables.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use cockpit_core::AgentRepo;
use cockpit_core::api::Provider;
use cockpit_i18n::{Msg, msg};
use serde::Deserialize;

mod file;
use file::*;

pub mod files_check;
pub mod location;
mod parse;

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
/// Upper bound of `max_prs_per_repo` and `max_prs_total` (each page is a call at GitHub).
const MAX_PRS_LIMIT: usize = 2000;

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
    /// `[github_projects]`: GitHub Projects as the source of tickets (`None` = Jira only).
    pub github_projects: Option<GitHubProjectsConfig>,
    /// `[github] auth`: the sign-in method the user picked (`None`: OAuth first, then token).
    pub github_auth: Option<cockpit_core::AuthChoice>,
    /// `[jira] auth`, as `github_auth`.
    pub jira_auth: Option<cockpit_core::AuthChoice>,
    pub github_oauth: Option<OAuthConfig>,
    pub atlassian: Option<AtlassianConfig>,
    /// Interval of the sync in seconds.
    pub interval_s: u64,
    /// What a workspace that has never been stored starts with.
    pub seed: Seed,
    /// `[agent] provider`: the tool of the agent.
    pub provider: Provider,
    /// `[agent.endpoint]`.
    pub endpoint: Option<EndpointConfig>,
    /// Chosen container runtime (`docker` or `podman`) from the file; `COCKPIT_CONTAINER`
    /// is in [`Config::container_env`].
    pub container: Option<String>,
    /// Value of `COCKPIT_CONTAINER`, goes before the file.
    pub container_env: Option<String>,
    /// `COCKPIT_AGENT_IO` (`bind` or `copy`): how folders get into the agent container; without it the
    /// server decides (copy inside the Cockpit container, bind otherwise).
    pub agent_io: Option<String>,
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
    /// `COCKPIT_BIND`: address the server listens on (environment only, never file). Default `127.0.0.1`;
    /// anything else is meant for the container image (`0.0.0.0`) and switches on `container_mode`.
    pub bind: IpAddr,
    /// `COCKPIT_PUBLIC_PORT`: the port under which the host reaches the published container port (`-p 127.0.0.1:18787:8787`),
    /// so `Host` and `Origin` with that port are accepted (environment only).
    pub public_port: Option<u16>,
    /// The file that was read from (and that the setup assistant writes to); `None`
    /// if there is no configuration directory.
    pub config_path: Option<PathBuf>,
    /// The folder `config/` in the project folder, if the default places are in use (checked by Git and permission rules).
    pub config_dir: Option<PathBuf>,
    /// `[server] webhooks`: the webhook routes answer (default `false`: 404).
    pub webhooks: bool,
    /// `[server] public_url`: address of the tunnel, only shown in the assistant.
    pub public_url: Option<String>,
    /// `[sync] webhook_fallback_interval_s`; `None` = [`Config::webhook_fallback_s`] decides.
    pub webhook_fallback_interval_s: Option<u64>,
    /// `[sync] max_prs_per_repo` and `max_prs_total`: how many open pull requests the sync loads.
    pub github_pr_limits: cockpit_integrations::github::PrLimits,
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
            "" | "empty" => Ok(Seed::Empty),
            "demo" => Ok(Seed::Demo),
            other => Err(ConfigError::InvalidValue(msg!(
                ServerConfigSeedUnknown,
                value = other
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
            github_projects: None,
            github_auth: None,
            jira_auth: None,
            github_oauth: None,
            atlassian: None,
            interval_s: DEFAULT_INTERVAL_S,
            seed: Seed::Empty,
            provider: Provider::default(),
            endpoint: None,
            container: None,
            container_env: None,
            agent_io: None,
            agent_repo: None,
            agent_repos: Vec::new(),
            agent_dir: None,
            git_base: None,
            dev_port: None,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            public_port: None,
            config_path: None,
            config_dir: None,
            webhooks: false,
            public_url: None,
            webhook_fallback_interval_s: None,
            github_pr_limits: Default::default(),
        }
    }

    /// Poll interval of a source while its webhooks arrive regularly.
    pub fn webhook_fallback_s(&self) -> u64 {
        fallback_s(self.interval_s, self.webhook_fallback_interval_s)
    }

    /// Loads the file (`COCKPIT_CONFIG` or default path; if it is missing the
    /// defaults apply) and puts the environment variables on top.
    pub fn load() -> Result<Self, ConfigError> {
        let env = |k: &str| std::env::var(k).ok();
        let project = location::project_dir();
        location::prepare(&project, &env)
            .map_err(|m| ConfigError::InvalidValue(cockpit_i18n::Msg::raw(m)))?;
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
        let mut c = Self::parse(text.as_deref(), env).map_err(|e| match &path {
            Some(p) => e.in_file(p),
            None => e,
        })?;
        c.config_path = path;
        let overrides = env("COCKPIT_CONFIG").is_some_and(|v| !v.is_empty())
            && env("COCKPIT_DB").is_some_and(|v| !v.is_empty());
        c.config_dir = (!overrides).then(|| location::config_dir(&project));
        Ok(c)
    }
}

#[cfg(test)]
mod tests;
