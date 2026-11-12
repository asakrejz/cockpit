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
//! intervall_s = 120                    # Abgleich alle 2 Minuten (mindestens 30)
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
//! anbieter = "claude-agent-sdk"        # umgesetzt; "claude-code" und "codex" sind bekannt, aber noch nicht umgesetzt
//! container = "podman"                 # oder "docker"; ohne Angabe erkennt das Backend die Laufzeit
//! repo = "firma/app"                   # Repo, in dem der Agent arbeitet (sonst das einzige aus [github] repos)
//! dir = "/pfad/zu/agent"               # Ordner mit Containerfile und Läufer (sonst ./apps/agent bzw. neben apps/cockpit/dist)
//!
//! [agent.endpunkt]
//! url = "https://litellm.example"      # Proxy oder kompatibler Endpunkt; leer oder weglassen = direkt bei Anthropic
//! modell = "claude-sonnet-5-5"         # Name, wie der Endpunkt ihn anbietet
//! ```
//!
//! Ältere Dateien mit `[litellm]` (`url`, `modell`) und dem Schlüssel `litellm` werden weiter gelesen
//! (gleichbedeutend mit `[agent.endpunkt]` und `agent-key`); steht beides da, gilt `[agent.endpunkt]`.
//! Der Assistent schreibt das neue Format.
//!
//! Eine Quelle gleicht nur ab, wenn sie hier eingerichtet ist **und** ihr Schlüssel im
//! Schlüsselbund liegt (`cockpit-server token setzen <github|jira>`). Ohne Quelle bleibt der
//! Workspace leer (oder zeigt die Beispieldaten, wenn der Seed `demo` gewählt ist).
//!
//! Umgebungsvariablen: `COCKPIT_PORT`, `COCKPIT_DIST`, `COCKPIT_DB`, `COCKPIT_CONFIG`
//! (Pfad der Konfigurationsdatei), `COCKPIT_SEED` (`leer` oder `demo`), `COCKPIT_CONTAINER`
//! (`docker` oder `podman`), `COCKPIT_AGENT_DIR` (Ordner `agent`), `COCKPIT_GIT_BASIS` (Basisadresse zum Klonen, nur für Tests),
//! `COCKPIT_DEV_PORT` (Port von `trunk serve`, nur für die Entwicklung).

use std::path::{Path, PathBuf};

use cockpit_core::api::Anbieter;
use serde::Deserialize;

pub const STANDARD_PORT: u16 = 8787;
pub const STANDARD_GITHUB_API: &str = "https://api.github.com";
/// So oft wird abgeglichen, wenn nichts eingestellt ist.
pub const STANDARD_INTERVALL_S: u64 = 120;
/// Kürzer geht nicht: Die Dienste begrenzen die Zahl der Abrufe.
pub const MIN_INTERVALL_S: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    /// Ordner der gebauten Web-App (`apps/cockpit/dist`).
    pub dist: PathBuf,
    pub db: PathBuf,
    pub github: Option<GitHubKonfig>,
    pub jira: Option<JiraKonfig>,
    pub github_oauth: Option<OAuthKonfig>,
    pub atlassian: Option<AtlassianKonfig>,
    /// Abstand des Abgleichs in Sekunden.
    pub intervall_s: u64,
    /// Womit ein Workspace beginnt, der noch nie gespeichert wurde.
    pub seed: Seed,
    /// `[agent] anbieter`: das Werkzeug des Agenten.
    pub anbieter: Anbieter,
    /// `[agent.endpunkt]` (oder das ältere `[litellm]`).
    pub endpunkt: Option<EndpunktKonfig>,
    /// Gewählte Container-Laufzeit (`docker` oder `podman`) aus der Datei; `COCKPIT_CONTAINER`
    /// steht in [`Config::container_env`].
    pub container: Option<String>,
    /// Wert von `COCKPIT_CONTAINER`, geht der Datei vor.
    pub container_env: Option<String>,
    /// `[agent] repo`: das Repo, in dem der Agent arbeitet (`owner/name`).
    pub agent_repo: Option<String>,
    /// `[agent] dir` bzw. `COCKPIT_AGENT_DIR`: der Ordner mit Containerfile und Läufer.
    pub agent_dir: Option<PathBuf>,
    /// `COCKPIT_GIT_BASIS`: Basisadresse zum Klonen statt `https://github.com` (für Tests).
    pub git_basis: Option<String>,
    /// `COCKPIT_DEV_PORT`: Port von `trunk serve` für die Entwicklung (nur Umgebung, nie Datei).
    pub dev_port: Option<u16>,
    /// Die Datei, aus der gelesen wurde (und in die der Einrichtungsassistent schreibt); `None`,
    /// wenn es kein Konfigurationsverzeichnis gibt.
    pub konfig_pfad: Option<PathBuf>,
}

/// Womit der Server startet, solange noch kein Stand gespeichert ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Seed {
    /// Ein leerer Workspace (Standard).
    #[default]
    Leer,
    /// Die Beispieldaten aus `cockpit_core::demo`.
    Demo,
}

impl Seed {
    fn aus_text(text: &str) -> Result<Self, KonfigFehler> {
        match text.trim().to_lowercase().as_str() {
            "" | "leer" => Ok(Seed::Leer),
            "demo" => Ok(Seed::Demo),
            anderes => Err(KonfigFehler::Wert(format!(
                "seed „{anderes}“ ist unbekannt, erlaubt sind leer und demo"
            ))),
        }
    }
}

/// Das Werkzeug und sein Endpunkt, wie zuletzt konfiguriert (im Betrieb austauschbar).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentWahl {
    pub anbieter: Anbieter,
    pub endpunkt: Option<EndpunktKonfig>,
}

/// Der Modell-Endpunkt des Agenten. Der Key liegt im Schlüsselbund, nie hier.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndpunktKonfig {
    /// Ohne Schrägstrich am Ende, z. B. `http://localhost:4000`; `None` = direkt beim Anbieter.
    pub url: Option<String>,
    pub modell: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubKonfig {
    pub api_url: String,
    /// `owner/name`; ohne Repos gleicht GitHub nicht ab.
    pub repos: Vec<String>,
}

/// Die GitHub App für die Anmeldung. Das Client-Secret liegt im Schlüsselbund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthKonfig {
    pub client_id: String,
}

/// Die Atlassian-App (OAuth 2.0 3LO). Das Client-Secret liegt im Schlüsselbund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlassianKonfig {
    pub client_id: String,
    /// Welche Site, falls mehrere freigegeben sind (Name, Adresse oder Id).
    pub site: Option<String>,
}

/// Mit OAuth sind `site` und `email` leer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraKonfig {
    pub site: String,
    pub email: String,
    pub projects: Vec<String>,
    pub jql: Option<String>,
}

#[derive(Debug)]
pub enum KonfigFehler {
    Lesen(PathBuf, std::io::Error),
    Toml(String),
    Wert(String),
}

impl std::fmt::Display for KonfigFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KonfigFehler::Lesen(p, e) => {
                write!(f, "Konfigurationsdatei {} nicht lesbar: {e}", p.display())
            }
            KonfigFehler::Toml(m) => write!(f, "Konfigurationsdatei fehlerhaft: {m}"),
            KonfigFehler::Wert(m) => write!(f, "Ungültiger Wert: {m}"),
        }
    }
}

impl std::error::Error for KonfigFehler {}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Datei {
    port: Option<u16>,
    dist: Option<PathBuf>,
    db: Option<PathBuf>,
    intervall_s: Option<u64>,
    github: Option<DateiGitHub>,
    jira: Option<DateiJira>,
    github_oauth: Option<DateiOAuth>,
    atlassian: Option<DateiAtlassian>,
    seed: Option<String>,
    litellm: Option<DateiEndpunkt>,
    agent: Option<DateiAgent>,
}

/// `[litellm]` (älter) und `[agent.endpunkt]` haben dieselben Felder.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiEndpunkt {
    url: Option<String>,
    modell: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiAgent {
    container: Option<String>,
    repo: Option<String>,
    dir: Option<PathBuf>,
    anbieter: Option<String>,
    endpunkt: Option<DateiEndpunkt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiOAuth {
    client_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiAtlassian {
    client_id: String,
    site: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiGitHub {
    api_url: Option<String>,
    #[serde(default)]
    repos: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiJira {
    #[serde(default)]
    site: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    projects: Vec<String>,
    jql: Option<String>,
}

/// `docker` oder `podman`, sonst ein Fehler.
fn laufzeit(text: &str) -> Result<String, KonfigFehler> {
    match text.trim().to_lowercase().as_str() {
        l @ ("docker" | "podman") => Ok(l.to_string()),
        anderes => Err(KonfigFehler::Wert(format!(
            "Container-Laufzeit „{anderes}“ wird nicht unterstützt, erlaubt sind docker und podman"
        ))),
    }
}

/// `owner/name` aus Buchstaben, Ziffern, `.`, `-` und `_`.
fn repo_gueltig(r: &str) -> bool {
    let teil = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    r.split_once('/')
        .is_some_and(|(o, n)| teil(o) && teil(n) && !n.contains('/'))
}

/// Standardpfad der Konfigurationsdatei.
pub fn standard_konfig_pfad() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cockpit").join("config.toml"))
}

fn standard_db_pfad() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cockpit")
        .join("cockpit.db")
}

impl Config {
    /// Nur Standardwerte.
    pub fn standard() -> Self {
        Self {
            port: STANDARD_PORT,
            dist: PathBuf::from("apps/cockpit/dist"),
            db: standard_db_pfad(),
            github: None,
            jira: None,
            github_oauth: None,
            atlassian: None,
            intervall_s: STANDARD_INTERVALL_S,
            seed: Seed::Leer,
            anbieter: Anbieter::default(),
            endpunkt: None,
            container: None,
            container_env: None,
            agent_repo: None,
            agent_dir: None,
            git_basis: None,
            dev_port: None,
            konfig_pfad: None,
        }
    }

    /// Standardwerte → Dateiinhalt → Umgebung. `datei` ist der Text der TOML-Datei (falls
    /// vorhanden), `umgebung` liefert Werte von Umgebungsvariablen.
    pub fn aus(
        datei: Option<&str>,
        umgebung: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, KonfigFehler> {
        let d: Datei = match datei {
            Some(text) => {
                toml::from_str(text).map_err(|e| KonfigFehler::Toml(e.message().to_string()))?
            }
            None => Datei::default(),
        };
        let mut c = Self::standard();
        if let Some(p) = d.port {
            c.port = p;
        }
        if let Some(p) = d.dist {
            c.dist = p;
        }
        if let Some(p) = d.db {
            c.db = p;
        }
        if let Some(i) = d.intervall_s {
            if i < MIN_INTERVALL_S {
                return Err(KonfigFehler::Wert(format!(
                    "intervall_s = {i} ist zu kurz, mindestens {MIN_INTERVALL_S} Sekunden"
                )));
            }
            c.intervall_s = i;
        }
        if let Some(g) = d.github {
            for r in &g.repos {
                if !repo_gueltig(r) {
                    return Err(KonfigFehler::Wert(format!(
                        "GitHub-Repo „{r}“ ist nicht von der Form owner/name"
                    )));
                }
            }
            c.github = Some(GitHubKonfig {
                api_url: g.api_url.unwrap_or_else(|| STANDARD_GITHUB_API.into()),
                repos: g.repos,
            });
        }
        let client_id = |wer: &str, id: String| {
            let id = id.trim().to_string();
            if id.is_empty() || id.chars().any(char::is_whitespace) {
                Err(KonfigFehler::Wert(format!(
                    "{wer}: client_id fehlt oder enthält Leerzeichen"
                )))
            } else {
                Ok(id)
            }
        };
        if let Some(o) = d.github_oauth {
            c.github_oauth = Some(OAuthKonfig {
                client_id: client_id("[github_oauth]", o.client_id)?,
            });
        }
        if let Some(a) = d.atlassian {
            c.atlassian = Some(AtlassianKonfig {
                client_id: client_id("[atlassian]", a.client_id)?,
                site: a
                    .site
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            });
        }
        // Mit Atlassian-Anmeldung genügt Jira ohne eigene Angaben.
        let jira = d.jira.or_else(|| {
            c.atlassian.as_ref().map(|_| DateiJira {
                site: String::new(),
                email: String::new(),
                projects: Vec::new(),
                jql: None,
            })
        });
        if let Some(j) = jira {
            if c.atlassian.is_none() && (j.site.trim().is_empty() || j.email.trim().is_empty()) {
                return Err(KonfigFehler::Wert(
                    "[jira] braucht site und email – oder [atlassian] für die Anmeldung per OAuth"
                        .into(),
                ));
            }
            for p in &j.projects {
                if cockpit_integrations::jira::jql_fuer(std::slice::from_ref(p), None).is_err() {
                    return Err(KonfigFehler::Wert(format!(
                        "Jira-Projekt „{p}“ ist kein gültiger Schlüssel"
                    )));
                }
            }
            c.jira = Some(JiraKonfig {
                site: j.site,
                email: j.email,
                projects: j.projects,
                jql: j.jql.filter(|q| !q.trim().is_empty()),
            });
        }

        if let Some(text) = d.seed {
            c.seed = Seed::aus_text(&text)?;
        }
        // Das ältere `[litellm]` gilt, solange `[agent.endpunkt]` fehlt.
        let mut endpunkt = d.litellm.map(|l| ("[litellm]", l));
        if let Some(a) = d.agent {
            if let Some(text) = a.anbieter {
                c.anbieter = Anbieter::aus_kennung(&text).ok_or_else(|| {
                    KonfigFehler::Wert(format!(
                        "[agent] anbieter „{}“ ist unbekannt, erlaubt sind claude-agent-sdk, claude-code und codex",
                        text.trim()
                    ))
                })?;
            }
            if let Some(e) = a.endpunkt {
                endpunkt = Some(("[agent.endpunkt]", e));
            }
            c.container = a.container.map(|t| laufzeit(&t)).transpose()?;
            c.agent_repo = a
                .repo
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty());
            if let Some(r) = &c.agent_repo
                && !crate::agent_git::repo_name_ok(r)
            {
                return Err(KonfigFehler::Wert(format!(
                    "[agent] repo „{r}“ muss die Form owner/name haben"
                )));
            }
            c.agent_dir = a.dir;
        }
        if let Some((wo, e)) = endpunkt {
            let url = e.url.map(|u| u.trim().trim_end_matches('/').to_string());
            let url = match url.filter(|u| !u.is_empty()) {
                None => None,
                Some(u) => {
                    let geht = u
                        .strip_prefix("http://")
                        .or_else(|| u.strip_prefix("https://"))
                        .is_some_and(|rest| {
                            !rest.is_empty() && !rest.contains(char::is_whitespace)
                        });
                    if !geht {
                        return Err(KonfigFehler::Wert(format!(
                            "{wo} url muss mit http:// oder https:// beginnen, z. B. http://localhost:4000"
                        )));
                    }
                    Some(u)
                }
            };
            let modell = e
                .modell
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty());
            if url.is_some() || modell.is_some() {
                c.endpunkt = Some(EndpunktKonfig { url, modell });
            }
        }
        if let Some(p) = umgebung("COCKPIT_AGENT_DIR").filter(|s| !s.trim().is_empty()) {
            c.agent_dir = Some(PathBuf::from(p.trim()));
        }
        if let Some(p) = umgebung("COCKPIT_GIT_BASIS").filter(|s| !s.trim().is_empty()) {
            c.git_basis = Some(p.trim().trim_end_matches('/').to_string());
        }

        if let Some(p) = umgebung("COCKPIT_SEED").filter(|s| !s.trim().is_empty()) {
            c.seed = Seed::aus_text(&p)?;
        }
        if let Some(p) = umgebung("COCKPIT_CONTAINER").filter(|s| !s.trim().is_empty()) {
            c.container_env = Some(laufzeit(&p)?);
        }
        if let Some(p) = umgebung("COCKPIT_DEV_PORT").filter(|s| !s.trim().is_empty()) {
            let nr: u16 = p.trim().parse().map_err(|_| {
                KonfigFehler::Wert(format!("COCKPIT_DEV_PORT={p} ist keine Portnummer"))
            })?;
            if nr == 0 {
                return Err(KonfigFehler::Wert(
                    "COCKPIT_DEV_PORT 0 ist nicht erlaubt".into(),
                ));
            }
            c.dev_port = Some(nr);
        }
        if let Some(p) = umgebung("COCKPIT_PORT").filter(|s| !s.is_empty()) {
            c.port = p.trim().parse().map_err(|_| {
                KonfigFehler::Wert(format!("COCKPIT_PORT={p} ist keine Portnummer"))
            })?;
        }
        if let Some(p) = umgebung("COCKPIT_DIST").filter(|s| !s.is_empty()) {
            c.dist = p.into();
        }
        if let Some(p) = umgebung("COCKPIT_DB").filter(|s| !s.is_empty()) {
            c.db = p.into();
        }
        if c.port == 0 {
            return Err(KonfigFehler::Wert("Port 0 ist nicht erlaubt".into()));
        }
        Ok(c)
    }

    /// Lädt die Datei (`COCKPIT_CONFIG` oder Standardpfad; fehlt sie, gelten die
    /// Standardwerte) und legt die Umgebungsvariablen darüber.
    pub fn laden() -> Result<Self, KonfigFehler> {
        let env = |k: &str| std::env::var(k).ok();
        let (pfad, ausdruecklich) = match env("COCKPIT_CONFIG").filter(|s| !s.is_empty()) {
            Some(p) => (Some(PathBuf::from(p)), true),
            None => (standard_konfig_pfad(), false),
        };
        let text = match pfad.as_deref().map(read_optional) {
            Some(Ok(t)) => t,
            Some(Err((p, e))) => return Err(KonfigFehler::Lesen(p, e)),
            None => None,
        };
        if ausdruecklich && text.is_none() {
            let p = pfad.unwrap_or_default();
            return Err(KonfigFehler::Lesen(p, std::io::ErrorKind::NotFound.into()));
        }
        let mut c = Self::aus(text.as_deref(), env)?;
        c.konfig_pfad = pfad;
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
mod tests {
    use super::*;

    fn keine_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn standardwerte() {
        let c = Config::aus(None, keine_env).unwrap();
        assert_eq!(c.port, 8787);
        assert_eq!(c.dist, PathBuf::from("apps/cockpit/dist"));
        assert!(c.db.ends_with("cockpit/cockpit.db"));
        assert!(c.github.is_none() && c.jira.is_none());
    }

    #[test]
    fn datei_ueberschreibt_standard() {
        let toml = r#"
            port = 9000
            dist = "/srv/app"
            [github]
            [jira]
            site = "https://firma.atlassian.net"
            email = "ich@firma.example"
        "#;
        let c = Config::aus(Some(toml), keine_env).unwrap();
        assert_eq!(c.port, 9000);
        assert_eq!(c.dist, PathBuf::from("/srv/app"));
        assert_eq!(c.github.unwrap().api_url, STANDARD_GITHUB_API);
        assert_eq!(c.jira.unwrap().email, "ich@firma.example");
        assert_eq!(c.intervall_s, 120);
    }

    #[test]
    fn quellen_und_intervall() {
        let toml = r#"
            intervall_s = 60
            [github]
            repos = ["firma/app", "firma/web.v2"]
            [jira]
            site = "https://firma.atlassian.net"
            email = "ich@firma.example"
            projects = ["BAU", "WEB"]
            jql = "  "
        "#;
        let c = Config::aus(Some(toml), keine_env).unwrap();
        assert_eq!(c.intervall_s, 60);
        assert_eq!(c.github.unwrap().repos, ["firma/app", "firma/web.v2"]);
        let j = c.jira.unwrap();
        assert_eq!(j.projects, ["BAU", "WEB"]);
        assert_eq!(j.jql, None, "leere Abfrage zählt nicht");
        for kaputt in [
            "intervall_s = 5",
            "[github]\nrepos = [\"nur-name\"]",
            "[github]\nrepos = [\"a/b/c\"]",
            "[github]\nrepos = [\"a b/c\"]",
            "[jira]\nsite = \"https://x\"\nemail = \"a@b\"\nprojects = [\"A) OR (B\"]",
        ] {
            assert!(
                matches!(
                    Config::aus(Some(kaputt), keine_env),
                    Err(KonfigFehler::Wert(_))
                ),
                "{kaputt}"
            );
        }
    }

    #[test]
    fn umgebung_ueberschreibt_datei() {
        let env = |k: &str| match k {
            "COCKPIT_PORT" => Some("9100".to_string()),
            "COCKPIT_DB" => Some("/tmp/x.db".to_string()),
            _ => None,
        };
        let c = Config::aus(Some("port = 9000"), env).unwrap();
        assert_eq!(c.port, 9100);
        assert_eq!(c.db, PathBuf::from("/tmp/x.db"));
    }

    #[test]
    fn fehler_sind_verstaendlich() {
        let env_kaputt = |k: &str| (k == "COCKPIT_PORT").then(|| "abc".to_string());
        assert!(matches!(
            Config::aus(None, env_kaputt),
            Err(KonfigFehler::Wert(_))
        ));
        assert!(matches!(
            Config::aus(Some("port = "), keine_env),
            Err(KonfigFehler::Toml(_))
        ));
        // Tippfehler in der Datei fallen auf, statt still ignoriert zu werden.
        assert!(matches!(
            Config::aus(Some("prot = 1"), keine_env),
            Err(KonfigFehler::Toml(_))
        ));
        assert!(matches!(
            Config::aus(Some("port = 0"), keine_env),
            Err(KonfigFehler::Wert(_))
        ));
        // Jira braucht Adresse und E-Mail.
        assert!(Config::aus(Some("[jira]\nsite = \"https://x\""), keine_env).is_err());
    }

    #[test]
    fn oauth_abschnitte() {
        let toml = r#"
            [github_oauth]
            client_id = " Iv23abc "
            [atlassian]
            client_id = "atl123"
            site = "firma"
        "#;
        let c = Config::aus(Some(toml), keine_env).unwrap();
        assert_eq!(c.github_oauth.unwrap().client_id, "Iv23abc");
        let a = c.atlassian.unwrap();
        assert_eq!(
            (a.client_id.as_str(), a.site.as_deref()),
            ("atl123", Some("firma"))
        );
        // [atlassian] allein richtet Jira ein, ohne site und email.
        let j = c.jira.unwrap();
        assert!(j.site.is_empty() && j.email.is_empty() && j.projects.is_empty());

        // Projekte bleiben einstellbar.
        let toml = "[atlassian]\nclient_id = \"a\"\n[jira]\nprojects = [\"BAU\"]";
        assert_eq!(
            Config::aus(Some(toml), keine_env)
                .unwrap()
                .jira
                .unwrap()
                .projects,
            ["BAU"]
        );
        for kaputt in [
            "[github_oauth]\nclient_id = \"\"",
            "[atlassian]\nclient_id = \"a b\"",
        ] {
            assert!(matches!(
                Config::aus(Some(kaputt), keine_env),
                Err(KonfigFehler::Wert(_))
            ));
        }
        // Tippfehler im Abschnitt fallen auf (Secrets gehören nicht in die Datei).
        assert!(matches!(
            Config::aus(
                Some("[atlassian]\nclient_id = \"a\"\nclient_secret = \"x\""),
                keine_env
            ),
            Err(KonfigFehler::Toml(_))
        ));
    }

    #[test]
    fn seed_ist_standardmaessig_leer() {
        assert_eq!(Config::aus(None, keine_env).unwrap().seed, Seed::Leer);
        assert_eq!(
            Config::aus(Some("seed = \"demo\""), keine_env)
                .unwrap()
                .seed,
            Seed::Demo
        );
        let env = |k: &str| (k == "COCKPIT_SEED").then(|| "leer".to_string());
        assert_eq!(
            Config::aus(Some("seed = \"demo\""), env).unwrap().seed,
            Seed::Leer,
            "die Umgebung geht der Datei vor"
        );
        let env = |k: &str| (k == "COCKPIT_SEED").then(|| " Demo ".to_string());
        assert_eq!(Config::aus(None, env).unwrap().seed, Seed::Demo);
        let env = |k: &str| (k == "COCKPIT_SEED").then(|| "viel".to_string());
        assert!(matches!(Config::aus(None, env), Err(KonfigFehler::Wert(_))));
        assert!(Config::aus(Some("seed = \"viel\""), keine_env).is_err());
    }

    #[test]
    fn repo_und_ordner_des_agenten() {
        let toml = "[agent]\nrepo = \" firma/app \"\ndir = \"/opt/cockpit/agent\"";
        let c = Config::aus(Some(toml), keine_env).unwrap();
        assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
        assert_eq!(c.agent_dir, Some(PathBuf::from("/opt/cockpit/agent")));
        let env = |k: &str| match k {
            "COCKPIT_AGENT_DIR" => Some("/andere/agent".to_string()),
            "COCKPIT_GIT_BASIS" => Some("file:///tmp/git/".to_string()),
            _ => None,
        };
        let c = Config::aus(Some(toml), env).unwrap();
        assert_eq!(c.agent_dir, Some(PathBuf::from("/andere/agent")));
        assert_eq!(c.git_basis.as_deref(), Some("file:///tmp/git"));
        for kaputt in ["repo = \"firma\"", "repo = \"../x\"", "repo = \"a/b/c\""] {
            assert!(
                matches!(
                    Config::aus(Some(&format!("[agent]\n{kaputt}")), keine_env),
                    Err(KonfigFehler::Wert(_))
                ),
                "{kaputt}"
            );
        }
        // Ohne Angabe nichts.
        let c = Config::aus(None, keine_env).unwrap();
        assert_eq!((c.agent_repo, c.agent_dir, c.git_basis), (None, None, None));
    }

    #[test]
    fn endpunkt_und_container() {
        let toml = r#"
            [agent]
            container = "Podman"
            anbieter = "Claude-Agent-SDK"
            [agent.endpunkt]
            url = " http://localhost:4000/ "
            modell = "claude-sonnet-5-5"
        "#;
        let c = Config::aus(Some(toml), keine_env).unwrap();
        assert_eq!(c.anbieter, Anbieter::ClaudeAgentSdk);
        let e = c.endpunkt.unwrap();
        assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
        assert_eq!(e.modell.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(c.container.as_deref(), Some("podman"));
        assert_eq!(c.container_env, None);

        let env = |k: &str| (k == "COCKPIT_CONTAINER").then(|| "docker".to_string());
        assert_eq!(
            Config::aus(None, env).unwrap().container_env.as_deref(),
            Some("docker")
        );
        for kaputt in [
            "[agent.endpunkt]\nurl = \"localhost:4000\"",
            "[agent.endpunkt]\nurl = \"http://\"",
            "[agent.endpunkt]\nurl = \"http://a b\"",
            "[agent]\ncontainer = \"nerdctl\"",
            "[agent]\nanbieter = \"gpt\"",
            "[litellm]\nurl = \"localhost:4000\"",
        ] {
            assert!(
                matches!(
                    Config::aus(Some(kaputt), keine_env),
                    Err(KonfigFehler::Wert(_))
                ),
                "{kaputt}"
            );
        }
        // Der Key gehört nicht in die Datei.
        for mit_key in [
            "[agent.endpunkt]\nurl = \"http://x\"\nkey = \"abc\"",
            "[litellm]\nurl = \"http://x\"\nkey = \"abc\"",
        ] {
            assert!(matches!(
                Config::aus(Some(mit_key), keine_env),
                Err(KonfigFehler::Toml(_))
            ));
        }
    }

    #[test]
    fn direkt_beim_anbieter_hat_keine_adresse() {
        let c = Config::aus(
            Some("[agent.endpunkt]\nmodell = \"claude-opus-5-5\""),
            keine_env,
        )
        .unwrap();
        let e = c.endpunkt.unwrap();
        assert_eq!(
            (e.url, e.modell.as_deref()),
            (None, Some("claude-opus-5-5"))
        );
        // Leere Adresse = direkt; ganz leerer Abschnitt = nicht eingerichtet.
        let c = Config::aus(
            Some("[agent.endpunkt]\nurl = \"\"\nmodell = \"m\""),
            keine_env,
        )
        .unwrap();
        assert_eq!(c.endpunkt.unwrap().url, None);
        assert_eq!(
            Config::aus(Some("[agent.endpunkt]"), keine_env)
                .unwrap()
                .endpunkt,
            None
        );
        assert_eq!(
            Config::aus(None, keine_env).unwrap().anbieter,
            Anbieter::ClaudeAgentSdk
        );
    }

    #[test]
    fn bekannte_werkzeuge_lassen_sich_eintragen() {
        for (text, soll) in [
            ("claude-code", Anbieter::ClaudeCode),
            ("codex", Anbieter::Codex),
        ] {
            let c =
                Config::aus(Some(&format!("[agent]\nanbieter = \"{text}\"")), keine_env).unwrap();
            assert_eq!(c.anbieter, soll);
        }
    }

    #[test]
    fn alte_litellm_datei_wird_weiter_gelesen_und_die_neue_geht_vor() {
        let alt = "[litellm]\nurl = \"http://localhost:4000/\"\nmodell = \"altes-modell\"\n[agent]\ncontainer = \"docker\"";
        let c = Config::aus(Some(alt), keine_env).unwrap();
        let e = c.endpunkt.unwrap();
        assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
        assert_eq!(e.modell.as_deref(), Some("altes-modell"));
        assert_eq!(c.anbieter, Anbieter::ClaudeAgentSdk);
        assert_eq!(c.container.as_deref(), Some("docker"));

        let neu = "[agent.endpunkt]\nurl = \"https://litellm.example\"\nmodell = \"neues-modell\"";
        let c = Config::aus(Some(neu), keine_env).unwrap();
        assert_eq!(
            c.endpunkt,
            Config::aus(Some(&format!("{alt}\n{neu}")), keine_env)
                .unwrap()
                .endpunkt
        );
        let e = c.endpunkt.unwrap();
        assert_eq!(e.url.as_deref(), Some("https://litellm.example"));
        assert_eq!(e.modell.as_deref(), Some("neues-modell"));
    }

    #[test]
    fn dev_port_nur_aus_der_umgebung() {
        assert_eq!(Config::aus(None, keine_env).unwrap().dev_port, None);
        let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "8081".to_string());
        assert_eq!(Config::aus(None, env).unwrap().dev_port, Some(8081));
        let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "x".to_string());
        assert!(Config::aus(None, env).is_err());
        assert!(Config::aus(Some("dev_port = 8081"), keine_env).is_err());
    }
}
