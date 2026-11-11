//! Konfiguration: Standardwerte, darüber die Datei, darüber Umgebungsvariablen.
//!
//! Die Datei (`config.toml` im Konfigurationsverzeichnis des Nutzers, z. B.
//! `~/.config/cockpit/`) enthält **keine Schlüssel** – nur Adressen und Konten.
//!
//! ```toml
//! port = 8787
//! dist = "/pfad/zu/cockpit/dist"
//! db = "/pfad/zur/cockpit.db"
//!
//! [github]
//! api_url = "https://api.github.com"   # optional
//!
//! [jira]
//! site = "https://firma.atlassian.net"
//! email = "ich@firma.example"
//! ```
//!
//! Umgebungsvariablen: `COCKPIT_PORT`, `COCKPIT_DIST`, `COCKPIT_DB`, `COCKPIT_CONFIG`
//! (Pfad der Konfigurationsdatei).

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const STANDARD_PORT: u16 = 8787;
pub const STANDARD_GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    /// Ordner der gebauten Web-App (`cockpit/dist`).
    pub dist: PathBuf,
    pub db: PathBuf,
    pub github: Option<GitHubKonfig>,
    pub jira: Option<JiraKonfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubKonfig {
    pub api_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraKonfig {
    pub site: String,
    pub email: String,
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
    github: Option<DateiGitHub>,
    jira: Option<DateiJira>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiGitHub {
    api_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DateiJira {
    site: String,
    email: String,
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
            dist: PathBuf::from("cockpit/dist"),
            db: standard_db_pfad(),
            github: None,
            jira: None,
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
        if let Some(g) = d.github {
            c.github = Some(GitHubKonfig {
                api_url: g.api_url.unwrap_or_else(|| STANDARD_GITHUB_API.into()),
            });
        }
        if let Some(j) = d.jira {
            c.jira = Some(JiraKonfig {
                site: j.site,
                email: j.email,
            });
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
        Self::aus(text.as_deref(), env)
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
        assert_eq!(c.dist, PathBuf::from("cockpit/dist"));
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
}
