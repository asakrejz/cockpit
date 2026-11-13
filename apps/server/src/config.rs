//! Konfiguration: Standardwerte, darüber die Datei, darüber Umgebungsvariablen.
//!
//! Die Datei (`config.toml` im Konfigurationsverzeichnis des Nutzers, z. B.
//! `~/.config/cockpit/`) enthält **keine Schlüssel** – nur Adressen und Konten.
//!
//! ```toml
//! port = 8787
//! dist = "/pfad/zu/apps/cockpit/dist"
//! db = "/pfad/zur/cockpit.db"
//!
//! interval_s = 120                     # Abgleich alle 2 Minuten (mindestens 30); älter: `intervall_s`
//!
//! [github]
//! api_url = "https://api.github.com"   # optional
//! repos = ["firma/app", "firma/web"]   # Repos für den Abgleich (owner/name)
//!
//! [jira]
//! site = "https://firma.atlassian.net"
//! email = "ich@firma.example"
//! projects = ["BAU", "WEB"]            # optional: nur diese Projekte
//! jql = "project = BAU AND ..."        # optional: eigene Abfrage statt der Projekte
//! ```
//!
//! Anmeldung per OAuth (statt eines API-Tokens), nur Client-IDs – die Secrets liegen im
//! Schlüsselbund (`cockpit-server token setzen github-oauth|atlassian-oauth`):
//!
//! ```toml
//! [github_oauth]
//! client_id = "Iv23li…"                # GitHub App
//!
//! [atlassian]
//! client_id = "7dNimh…"                # OAuth-2.0-3LO-App
//! site = "firma"                       # optional: Name, Adresse oder Id, falls mehrere freigegeben sind
//! ```
//!
//! Mit `[atlassian]` braucht `[jira]` weder `site` noch `email` (nur noch `projects`/`jql`).
//!
//! Agent-Anbieter und Endpunkt (nur Adresse und Modellname – der Key liegt im Schlüsselbund,
//! `cockpit-server token setzen agent-key`), Container-Laufzeit und Seed:
//!
//! ```toml
//! seed = "demo"                        # Beispieldaten, nur wenn noch kein Stand gespeichert ist
//!
//! [agent]
//! provider = "claude-agent-sdk"        # umgesetzt; "claude-code" und "codex" sind bekannt, aber noch nicht umgesetzt
//! container = "podman"                 # oder "docker"; ohne Angabe erkennt das Backend die Laufzeit
//! repo = "firma/app"                   # Repo, in dem der Agent arbeitet (sonst das einzige aus [github] repos)
//! dir = "/pfad/zu/agent"               # Ordner mit Containerfile und Läufer (sonst ./apps/agent bzw. neben apps/cockpit/dist)
//!
//! [agent.endpoint]
//! url = "https://litellm.example"      # Proxy oder kompatibler Endpunkt; leer oder weglassen = direkt bei Anthropic
//! model = "claude-sonnet-5-5"          # Name, wie der Endpunkt ihn anbietet
//! ```
//!
//! Ältere Dateien werden weiter gelesen: `[litellm]` (`url`, `modell`) und der Schlüssel `litellm`
//! (gleichbedeutend mit `[agent.endpoint]` und `agent-key`), `[agent] anbieter`, `[agent.endpunkt]`
//! mit `modell` und `intervall_s`; steht beides da, gilt das neue.
//! Der Assistent schreibt das neue Format.
//!
//! Eine Quelle gleicht nur ab, wenn sie hier eingerichtet ist **und** ihr Schlüssel im
//! Schlüsselbund liegt (`cockpit-server token setzen <github|jira>`). Ohne Quelle bleibt der
//! Workspace leer (oder zeigt die Beispieldaten, wenn der Seed `demo` gewählt ist).
//!
//! Umgebungsvariablen: `COCKPIT_PORT`, `COCKPIT_DIST`, `COCKPIT_DB`, `COCKPIT_CONFIG`
//! (Pfad der Konfigurationsdatei), `COCKPIT_SEED` (`empty` oder `demo`; `leer` gilt weiter), `COCKPIT_CONTAINER`
//! (`docker` oder `podman`), `COCKPIT_AGENT_DIR` (Ordner `agent`), `COCKPIT_GIT_BASE` (älter `COCKPIT_GIT_BASIS`; Basisadresse zum Klonen, nur für Tests),
//! `COCKPIT_DEV_PORT` (Port von `trunk serve`, nur für die Entwicklung).

use std::path::{Path, PathBuf};

use cockpit_core::api::Provider;
use serde::Deserialize;

pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_GITHUB_API: &str = "https://api.github.com";
/// So oft wird abgeglichen, wenn nichts eingestellt ist.
pub const DEFAULT_INTERVAL_S: u64 = 120;
/// Kürzer geht nicht: Die Dienste begrenzen die Zahl der Abrufe.
pub const MIN_INTERVAL_S: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    /// Ordner der gebauten Web-App (`apps/cockpit/dist`).
    pub dist: PathBuf,
    pub db: PathBuf,
    pub github: Option<GitHubConfig>,
    pub jira: Option<JiraConfig>,
    pub github_oauth: Option<OAuthConfig>,
    pub atlassian: Option<AtlassianConfig>,
    /// Abstand des Abgleichs in Sekunden.
    pub interval_s: u64,
    /// Womit ein Workspace beginnt, der noch nie gespeichert wurde.
    pub seed: Seed,
    /// `[agent] anbieter`: das Werkzeug des Agenten.
    pub provider: Provider,
    /// `[agent.endpunkt]` (oder das ältere `[litellm]`).
    pub endpoint: Option<EndpointConfig>,
    /// Gewählte Container-Laufzeit (`docker` oder `podman`) aus der Datei; `COCKPIT_CONTAINER`
    /// steht in [`Config::container_env`].
    pub container: Option<String>,
    /// Wert von `COCKPIT_CONTAINER`, geht der Datei vor.
    pub container_env: Option<String>,
    /// `[agent] repo`: das Repo, in dem der Agent arbeitet (`owner/name`).
    pub agent_repo: Option<String>,
    /// `[agent] dir` bzw. `COCKPIT_AGENT_DIR`: der Ordner mit Containerfile und Läufer.
    pub agent_dir: Option<PathBuf>,
    /// `COCKPIT_GIT_BASE`: Basisadresse zum Klonen statt `https://github.com` (für Tests).
    pub git_base: Option<String>,
    /// `COCKPIT_DEV_PORT`: Port von `trunk serve` für die Entwicklung (nur Umgebung, nie Datei).
    pub dev_port: Option<u16>,
    /// Die Datei, aus der gelesen wurde (und in die der Einrichtungsassistent schreibt); `None`,
    /// wenn es kein Konfigurationsverzeichnis gibt.
    pub config_path: Option<PathBuf>,
}

/// Womit der Server startet, solange noch kein Stand gespeichert ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Seed {
    /// Ein leerer Workspace (Standard).
    #[default]
    Empty,
    /// Die Beispieldaten aus `cockpit_core::demo`.
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

/// Das Werkzeug und sein Endpunkt, wie zuletzt konfiguriert (im Betrieb austauschbar).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentChoice {
    pub provider: Provider,
    pub endpoint: Option<EndpointConfig>,
}

/// Der Modell-Endpunkt des Agenten. Der Key liegt im Schlüsselbund, nie hier.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndpointConfig {
    /// Ohne Schrägstrich am Ende, z. B. `http://localhost:4000`; `None` = direkt beim Anbieter.
    pub url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubConfig {
    pub api_url: String,
    /// `owner/name`; ohne Repos gleicht GitHub nicht ab.
    pub repos: Vec<String>,
}

/// Die GitHub App für die Anmeldung. Das Client-Secret liegt im Schlüsselbund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
}

/// Die Atlassian-App (OAuth 2.0 3LO). Das Client-Secret liegt im Schlüsselbund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlassianConfig {
    pub client_id: String,
    /// Welche Site, falls mehrere freigegeben sind (Name, Adresse oder Id).
    pub site: Option<String>,
}

/// Mit OAuth sind `site` und `email` leer.
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
                write!(f, "Konfigurationsdatei {} nicht lesbar: {e}", p.display())
            }
            ConfigError::Toml(m) => write!(f, "Konfigurationsdatei fehlerhaft: {m}"),
            ConfigError::Wert(m) => write!(f, "Ungültiger Wert: {m}"),
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

/// `[litellm]` (älter) und `[agent.endpunkt]` haben dieselben Felder.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEndpoint {
    url: Option<String>,
    #[serde(alias = "modell")]
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAgent {
    container: Option<String>,
    repo: Option<String>,
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

/// `docker` oder `podman`, sonst ein Fehler.
fn runtime(text: &str) -> Result<String, ConfigError> {
    match text.trim().to_lowercase().as_str() {
        l @ ("docker" | "podman") => Ok(l.to_string()),
        other => Err(ConfigError::Wert(format!(
            "Container-Laufzeit „{other}“ wird nicht unterstützt, erlaubt sind docker und podman"
        ))),
    }
}

/// `owner/name` aus Buchstaben, Ziffern, `.`, `-` und `_`.
fn repo_valid(r: &str) -> bool {
    let teil = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    r.split_once('/')
        .is_some_and(|(o, n)| teil(o) && teil(n) && !n.contains('/'))
}

/// Standardpfad der Konfigurationsdatei.
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
    /// Nur Standardwerte.
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
            agent_dir: None,
            git_base: None,
            dev_port: None,
            config_path: None,
        }
    }

    /// Standardwerte → Dateiinhalt → Umgebung. `datei` ist der Text der TOML-Datei (falls
    /// vorhanden), `umgebung` liefert Werte von Umgebungsvariablen.
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
        // Mit Atlassian-Anmeldung genügt Jira ohne eigene Angaben.
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
        // Das ältere `[litellm]` gilt, solange `[agent.endpunkt]` fehlt.
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

    /// Lädt die Datei (`COCKPIT_CONFIG` oder Standardpfad; fehlt sie, gelten die
    /// Standardwerte) und legt die Umgebungsvariablen darüber.
    pub fn laden() -> Result<Self, ConfigError> {
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

/// `Ok(None)`, wenn die Datei nicht existiert.
fn read_optional(p: &Path) -> Result<Option<String>, (PathBuf, std::io::Error)> {
    match std::fs::read_to_string(p) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err((p.to_path_buf(), e)),
    }
}

#[cfg(test)]
mod tests;
