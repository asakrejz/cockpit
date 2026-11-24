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
//! interval_s = 120                     # sync every 2 minutes (at least 30)
//!
//! [github]
//! api_url = "https://api.github.com"   # optional
//! repos = ["acme/app", "acme/web"]     # repos for the sync (owner/name)
//!
//! [jira]
//! site = "https://acme.atlassian.net"
//! email = "me@acme.example"
//! projects = ["BAU", "WEB"]            # optional: only these projects
//! jql = "project = BAU AND ..."        # optional: own query instead of the projects
//! hidden_fields = ["Rank", "customfield_10099"]   # optional: fields (name or ID) the ticket detail leaves out
//! field_order = ["Acceptance criteria"]           # optional: fields (name or ID) the detail shows first
//! team_field = "customfield_10400"                # optional: the ID of the „Team“ field, if the detection fails
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
//! site = "acme"                        # optional: name, address or id, if several are approved
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
//! repo = "acme/app"                    # default repo of the agent (otherwise the only one from [github] repos)
//! dir = "/path/to/agent"               # folder with Containerfile and runner (otherwise ./apps/agent or next to apps/cockpit/dist)
//!
//! # Settings per repo. The agent starts in the default repo (`repo` above, or the only repo
//! # listed); the interface lets you choose another one before the start.
//! [[agent.repos]]
//! repo = "acme/web"
//! base_branch = "develop"              # optional, default "main"
//! branch_pattern = "feature/{ticket}-{slug}"   # optional, default "claude/{ticket}-{slug}"
//! pr_language = "de"                           # optional language of PR title and text, default "en"
//!
//! [[agent.repos]]
//! repo = "acme/app"
//!
//! [agent.endpoint]
//! url = "https://litellm.example"      # proxy or compatible endpoint; empty or omitted = directly at Anthropic
//! model = "claude-sonnet-5-5"          # name as the endpoint offers it
//! ```
//!
//! GitHub Projects as the source of tickets (instead of or next to Jira):
//!
//! ```toml
//! [github_projects]
//! tickets_from = "github_projects"     # "jira", "github_projects" or "both" (default "both" when the section exists)
//! projects = ["acme/3", "acme/1"]      # owner/number as in the project's address
//! status_field = "Status"              # optional: the single-select field that counts as the ticket status
//! team_field = "Team"                  # optional: the field that holds the team
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
//! max_prs_per_repo = 200                   # open pull requests loaded per repo (default 200)
//! max_prs_total = 500                      # … and over all repos together (default 500)
//! ```
//!
//! A key that does not exist in this list is an error at start (the message names the key, the
//! section and the file), never silently ignored.
//!
//! A source only syncs if it is set up here **and** its key is in the
//! .env file (`cockpit-server token set <github|jira>`). Without a source the
//! workspace stays empty (or shows the sample data if the seed `demo` is chosen).
//!
//! Environment variables: `COCKPIT_PORT`, `COCKPIT_DIST`, `COCKPIT_DB`, `COCKPIT_CONFIG`
//! (path of the configuration file), `COCKPIT_SEED` (`empty` or `demo`), `COCKPIT_CONTAINER`
//! (`docker` or `podman`), `COCKPIT_AGENT_DIR` (folder `agent`), `COCKPIT_GIT_BASE` (base address for cloning, tests only),
//! `COCKPIT_BIND` (listen address; default `127.0.0.1`, `0.0.0.0` only inside the container image) and `COCKPIT_PUBLIC_PORT` (published host port if it differs),
//! `COCKPIT_DEV_PORT` (port of `trunk serve`, for development only), `COCKPIT_ENV_FILE` (path of the `.env` file with the keys, default `config/.env`).

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
            other => Err(ConfigError::Wert(msg!(
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

    /// Defaults → file content → environment. `file` is the text of the TOML file (if
    /// present), `env` delivers values of environment variables.
    pub fn parse(
        file: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let d: File = match file {
            Some(text) => file_read(text)?,
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
                return Err(ConfigError::Wert(msg!(
                    ServerConfigIntervalShort,
                    interval = i,
                    min = MIN_INTERVAL_S
                )));
            }
            c.interval_s = i;
        }
        if let Some(g) = d.github {
            c.github_auth = g.auth;
            for r in &g.repos {
                if !cockpit_core::repo_name_valid(r) {
                    return Err(ConfigError::Wert(msg!(
                        ServerConfigGithubRepoForm,
                        repo = r
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
                Err(ConfigError::Wert(msg!(
                    ServerConfigClientIdInvalid,
                    who = wer
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
                hidden_fields: Vec::new(),
                field_order: Vec::new(),
                team_field: None,
                auth: None,
            })
        });
        if let Some(j) = jira {
            c.jira_auth = j.auth;
            if c.atlassian.is_none() && (j.site.trim().is_empty() || j.email.trim().is_empty()) {
                return Err(ConfigError::Wert(msg!(ServerConfigJiraNeedsSite)));
            }
            for p in &j.projects {
                if cockpit_integrations::jira::jql_for(std::slice::from_ref(p), None).is_err() {
                    return Err(ConfigError::Wert(msg!(
                        ServerConfigJiraProjectInvalid,
                        project = p
                    )));
                }
            }
            c.jira = Some(JiraConfig {
                site: j.site,
                email: j.email,
                projects: j.projects,
                jql: j.jql.filter(|q| !q.trim().is_empty()),
                hidden_fields: clean_list(j.hidden_fields),
                field_order: clean_list(j.field_order),
                team_field: j
                    .team_field
                    .map(|f| f.trim().to_string())
                    .filter(|f| !f.is_empty()),
            });
        }

        if let Some(p) = d.github_projects {
            let tickets_from = match p.tickets_from.as_deref().map(str::trim) {
                None | Some("") => cockpit_core::TicketsFrom::Both,
                Some(t) => cockpit_core::TicketsFrom::from_id(t).ok_or_else(|| {
                    ConfigError::Wert(msg!(ServerConfigTicketsFromUnknown, value = t))
                })?,
            };
            let mut projects = Vec::new();
            for text in &p.projects {
                let r = cockpit_core::ProjectRef::parse(text).ok_or_else(|| {
                    ConfigError::Wert(msg!(ServerConfigProjectForm, project = text))
                })?;
                if !projects.contains(&r) {
                    projects.push(r);
                }
            }
            let name =
                |v: Option<String>| v.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
            c.github_projects = Some(GitHubProjectsConfig {
                tickets_from,
                projects,
                status_field: name(p.status_field),
                team_field: name(p.team_field),
            });
        }

        if let Some(text) = d.seed {
            c.seed = Seed::from_text(&text)?;
        }
        let mut endpoint = None;
        if let Some(a) = d.agent {
            if let Some(text) = a.provider {
                c.provider = Provider::from_id(&text).ok_or_else(|| {
                    ConfigError::Wert(msg!(ServerConfigProviderUnknown, provider = text.trim()))
                })?;
            }
            endpoint = a.endpoint;
            c.container = a.container.map(|t| runtime(&t)).transpose()?;
            c.agent_repo = a
                .repo
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty());
            if let Some(r) = &c.agent_repo
                && !cockpit_core::repo_name_valid(r)
            {
                return Err(ConfigError::Wert(msg!(ServerConfigAgentRepoForm, repo = r)));
            }
            c.agent_repos = agent_repos_read(&a.repos)?;
            c.agent_dir = a.dir;
        }
        // Watched repos (all open pull requests are loaded): `[github] repos` plus the entries of
        // `[[agent.repos]]` (older configurations; an entry the app stored on the first use of a
        // repo has `watch = false` and only keeps its settings). Everything else is found by the
        // search, whatever repos exist.
        if c.agent_repos.iter().any(|r| r.watch) {
            let g = c.github.get_or_insert_with(GitHubConfig::default);
            for r in c.agent_repos.iter().filter(|r| r.watch) {
                if !g.repos.contains(&r.repo) {
                    g.repos.push(r.repo.clone());
                }
            }
        }
        if let Some(e) = endpoint {
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
                        return Err(ConfigError::Wert(msg!(ServerConfigEndpointUrlScheme)));
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
        if let Some(s) = &d.sync {
            for (name, value) in [
                ("max_prs_per_repo", s.max_prs_per_repo),
                ("max_prs_total", s.max_prs_total),
            ] {
                if let Some(n) = value
                    && !(1..=MAX_PRS_LIMIT).contains(&n)
                {
                    return Err(ConfigError::Wert(msg!(
                        ServerConfigSyncLimit,
                        name = name,
                        value = n,
                        max = MAX_PRS_LIMIT
                    )));
                }
            }
            if let Some(n) = s.max_prs_per_repo {
                c.github_pr_limits.per_repo = n;
            }
            if let Some(n) = s.max_prs_total {
                c.github_pr_limits.total = n;
            }
        }
        if let Some(i) = d.sync.and_then(|s| s.webhook_fallback_interval_s) {
            if !(c.interval_s..=WEBHOOK_FALLBACK_LIMIT_S).contains(&i) {
                return Err(ConfigError::Wert(msg!(
                    ServerConfigWebhookFallbackRange,
                    value = i,
                    interval = c.interval_s,
                    max = WEBHOOK_FALLBACK_LIMIT_S
                )));
            }
            c.webhook_fallback_interval_s = Some(i);
        }
        if let Some(p) = env("COCKPIT_AGENT_DIR").filter(|s| !s.trim().is_empty()) {
            c.agent_dir = Some(PathBuf::from(p.trim()));
        }
        if let Some(p) = env("COCKPIT_GIT_BASE").filter(|s| !s.trim().is_empty()) {
            c.git_base = Some(p.trim().trim_end_matches('/').to_string());
        }

        if let Some(p) = env("COCKPIT_SEED").filter(|s| !s.trim().is_empty()) {
            c.seed = Seed::from_text(&p)?;
        }
        if let Some(p) = env("COCKPIT_CONTAINER").filter(|s| !s.trim().is_empty()) {
            c.container_env = Some(runtime(&p)?);
        }
        if let Some(p) = env("COCKPIT_AGENT_IO").filter(|s| !s.trim().is_empty()) {
            let v = p.trim().to_ascii_lowercase();
            if !matches!(v.as_str(), "bind" | "copy") {
                return Err(ConfigError::Wert(msg!(
                    ServerConfigAgentIoInvalid,
                    value = &p
                )));
            }
            c.agent_io = Some(v);
        }
        if let Some(p) = env("COCKPIT_DEV_PORT").filter(|s| !s.trim().is_empty()) {
            let nr: u16 = p
                .trim()
                .parse()
                .map_err(|_| ConfigError::Wert(msg!(ServerConfigDevPortInvalid, value = &p)))?;
            if nr == 0 {
                return Err(ConfigError::Wert(msg!(ServerConfigDevPortZero)));
            }
            c.dev_port = Some(nr);
        }
        if let Some(b) = env("COCKPIT_BIND").filter(|s| !s.trim().is_empty()) {
            c.bind = b
                .trim()
                .parse()
                .map_err(|_| ConfigError::Wert(msg!(ServerConfigBindInvalid, value = &b)))?;
        }
        if let Some(p) = env("COCKPIT_PUBLIC_PORT").filter(|s| !s.trim().is_empty()) {
            let nr: u16 = p.trim().parse().ok().filter(|n| *n != 0).ok_or_else(|| {
                ConfigError::Wert(msg!(ServerConfigPublicPortInvalid, value = &p))
            })?;
            c.public_port = Some(nr);
        }
        if let Some(p) = env("COCKPIT_PORT").filter(|s| !s.is_empty()) {
            c.port = p
                .trim()
                .parse()
                .map_err(|_| ConfigError::Wert(msg!(ServerConfigPortInvalid, value = &p)))?;
        }
        if let Some(p) = env("COCKPIT_DIST").filter(|s| !s.is_empty()) {
            c.dist = p.into();
        }
        if let Some(p) = env("COCKPIT_DB").filter(|s| !s.is_empty()) {
            c.db = p.into();
        }
        if c.port == 0 {
            return Err(ConfigError::Wert(msg!(ServerConfigPortZero)));
        }
        Ok(c)
    }

    /// Loads the file (`COCKPIT_CONFIG` or default path; if it is missing the
    /// defaults apply) and puts the environment variables on top.
    pub fn load() -> Result<Self, ConfigError> {
        let env = |k: &str| std::env::var(k).ok();
        let project = location::project_dir();
        location::prepare(&project, &env)
            .map_err(|m| ConfigError::Wert(cockpit_i18n::Msg::raw(m)))?;
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

/// Trims a list of names and drops the empty ones.
fn clean_list(list: Vec<String>) -> Vec<String> {
    list.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
