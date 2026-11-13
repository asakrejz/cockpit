//! Configuration: defaults, above them the file, above that environment variables.
//!
//! The file (`config.toml` in the configuration directory of the user, e.g.
//! `~/.config/cockpit/`) contains **no keys** – only addresses and accounts.
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
//! provider = "claude-agent-sdk"        # implemented; "claude-code" and "codex" are known but not implemented yet
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

pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_GITHUB_API: &str = "https://api.github.com";
/// This often a sync happens if nothing is set.
pub const DEFAULT_INTERVAL_S: u64 = 120;
/// Shorter is not possible: the services limit the number of fetches.
pub const MIN_INTERVAL_S: u64 = 30;

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

/// The tool and its endpoint as last configured (replaceable during operation).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentChoice {
    pub provider: Provider,
    pub endpoint: Option<EndpointConfig>,
}

/// The model endpoint of the agent. The key is in the .env file, never here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndpointConfig {
    /// Without a slash at the end, e.g. `http://localhost:4000`; `None` = directly at the provider.
    pub url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubConfig {
    pub api_url: String,
    /// `owner/name`; without repos GitHub does not sync.
    pub repos: Vec<String>,
}

/// The GitHub App for the sign-in. The client secret is in the .env file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
}

/// The Atlassian app (OAuth 2.0 3LO). The client secret is in the .env file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlassianConfig {
    pub client_id: String,
    /// Which site, if several are approved (name, address or id).
    pub site: Option<String>,
}

/// With OAuth `site` and `email` are empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraConfig {
    pub site: String,
    pub email: String,
    pub projects: Vec<String>,
    pub jql: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, std::io::Error),
    Toml(String),
    Wert(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read(p, e) => {
                write!(f, "Configuration file {} not readable: {e}", p.display())
            }
            ConfigError::Toml(m) => write!(f, "Configuration file invalid: {m}"),
            ConfigError::Wert(m) => write!(f, "Invalid value: {m}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    port: Option<u16>,
    dist: Option<PathBuf>,
    db: Option<PathBuf>,
    #[serde(alias = "intervall_s")]
    interval_s: Option<u64>,
    github: Option<FileGitHub>,
    jira: Option<FileJira>,
    github_oauth: Option<FileOAuth>,
    atlassian: Option<FileAtlassian>,
    seed: Option<String>,
    litellm: Option<FileEndpoint>,
    agent: Option<FileAgent>,
}

/// `[litellm]` (older) and `[agent.endpoint]` have the same fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEndpoint {
    url: Option<String>,
    #[serde(alias = "modell")]
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAgentRepo {
    repo: String,
    base_branch: Option<String>,
    #[serde(default)]
    projects: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAgent {
    container: Option<String>,
    repo: Option<String>,
    #[serde(default)]
    repos: Vec<FileAgentRepo>,
    dir: Option<PathBuf>,
    #[serde(alias = "anbieter")]
    provider: Option<String>,
    #[serde(alias = "endpunkt")]
    endpoint: Option<FileEndpoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileOAuth {
    client_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAtlassian {
    client_id: String,
    site: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileGitHub {
    api_url: Option<String>,
    #[serde(default)]
    repos: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileJira {
    #[serde(default)]
    site: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    projects: Vec<String>,
    jql: Option<String>,
}

/// `docker` or `podman`, otherwise an error.
fn runtime(text: &str) -> Result<String, ConfigError> {
    match text.trim().to_lowercase().as_str() {
        l @ ("docker" | "podman") => Ok(l.to_string()),
        other => Err(ConfigError::Wert(format!(
            "Container-Laufzeit „{other}“ wird nicht unterstützt, erlaubt sind docker und podman"
        ))),
    }
}

/// `owner/name` made of letters, digits, `.`, `-` and `_`.
fn repo_valid(r: &str) -> bool {
    let teil = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    r.split_once('/')
        .is_some_and(|(o, n)| teil(o) && teil(n) && !n.contains('/'))
}

/// `[[agent.repos]]`: every repo valid and once, every project a valid key and routed to one
/// repo only (otherwise the rule would be ambiguous).
fn agent_repos_read(list: &[FileAgentRepo]) -> Result<Vec<AgentRepo>, ConfigError> {
    let mut out: Vec<AgentRepo> = Vec::new();
    for r in list {
        let repo = r.repo.trim().to_string();
        if !crate::agent_git::repo_name_ok(&repo) {
            return Err(ConfigError::Wert(format!(
                "[[agent.repos]] repo „{repo}“ muss die Form owner/name haben"
            )));
        }
        if out.iter().any(|o| o.repo == repo) {
            return Err(ConfigError::Wert(format!(
                "[[agent.repos]] nennt das Repo „{repo}“ mehrfach"
            )));
        }
        let base = r
            .base_branch
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .unwrap_or(cockpit_core::repos::DEFAULT_BASE);
        if !crate::agent_git::base_branch_ok(base) {
            return Err(ConfigError::Wert(format!(
                "[[agent.repos]] base_branch „{base}“ von {repo} ist kein gültiger Branch-Name"
            )));
        }
        let mut projects: Vec<String> = Vec::new();
        for p in &r.projects {
            let p = p.trim().to_ascii_uppercase();
            if !is_project_key(&p) {
                return Err(ConfigError::Wert(format!(
                    "[[agent.repos]] {repo}: „{p}“ ist kein gültiger Jira-Projektschlüssel"
                )));
            }
            if let Some(other) = out.iter().find(|o| o.projects.contains(&p)) {
                return Err(ConfigError::Wert(format!(
                    "Das Jira-Projekt {p} ist schon dem Repo „{}“ zugeordnet, nicht noch {repo}",
                    other.repo
                )));
            }
            if !projects.contains(&p) {
                projects.push(p);
            }
        }
        out.push(AgentRepo {
            repo,
            base_branch: base.to_string(),
            projects,
        });
    }
    Ok(out)
}

/// Default path of the configuration file.
pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cockpit").join("config.toml"))
}

fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cockpit")
        .join("cockpit.db")
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
        }
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
        let (path, explicit) = match env("COCKPIT_CONFIG").filter(|s| !s.is_empty()) {
            Some(p) => (Some(PathBuf::from(p)), true),
            None => (default_config_path(), false),
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
        Ok(c)
    }
}

/// `Ok(None)` if the file does not exist.
fn read_optional(p: &Path) -> Result<Option<String>, (PathBuf, std::io::Error)> {
    match std::fs::read_to_string(p) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err((p.to_path_buf(), e)),
    }
}

#[cfg(test)]
mod tests;
