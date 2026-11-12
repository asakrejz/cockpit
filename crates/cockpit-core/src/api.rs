//! Die JSON-Formen zwischen App und Backend (Server-Modus).
//!
//! * `GET /api/workspace`: der Stand in der Form von [`persist`](crate::persist), dazu `revision`
//! * `POST /api/actions`: Body [`ActionRequest`], Antwort [`ActionResponse`] oder [`ErrorBody`]
//! * `GET /api/events`: Server-Sent Events, jedes Ereignis trägt ein [`ServerEvent`]
//! * `GET /api/oauth/{github|atlassian}/start` und `…/callback`: Anmeldung per OAuth (Navigation des
//!   Browsers, leitet nach `/` um); `POST …/widerrufen`: [`WiderrufAntwort`]
//! * `GET /api/jira/projekte`: die Jira-Projekte für die Auswahl, [`JiraProjectsResponse`]
//! * `GET /api/agent/modelle`: die Modell-IDs des Endpunkts für das Drop-down, [`Modellliste`]
//! * `GET /api/agent/pruefen` (Alias `GET /api/litellm/pruefen`): Verbindungstest zum Endpunkt, [`EndpunktPruefung`]
//! * `POST /api/jira/pruefen`: Verbindungstest für Jira per API-Token (nur Lesen), [`JiraPruefung`]
//! * `DELETE /api/schluessel/{name}`: löscht einen Token (`jira`, `github`) aus dem Schlüsselbund
//! * `GET /api/einrichtung`: was der Einrichtungsassistent vorbelegt, [`Einrichtung`]
//! * `PUT /api/konfiguration`: Body [`KonfigAenderung`], schreibt `config.toml`, Antwort [`KonfigAntwort`]
//! * `PUT /api/schluessel/{name}`: Body [`SchluesselEingabe`], legt den Schlüssel im Schlüsselbund
//!   ab, Antwort [`SchluesselAntwort`] (nur „gesetzt“, nie der Wert)
//! * `GET /api/agent/image` / `POST /api/agent/image`: Stand bzw. Start von „Image bauen“, [`ImageBau`]
//! * `GET /api/agent/pruefen/container` (Alias `GET /api/litellm/pruefen/container`): dieselbe Prüfung aus einem Container, [`ContainerPruefung`]
//! * `GET /api/health`: unter `agent` ein [`AgentStatus`], unter `endpunkt` ein [`EndpunktStatus`], unter `container` ein [`ContainerStatus`]
//!
//! Beide Seiten benutzen diese Typen, damit die Formen nicht auseinanderlaufen.

use serde::{Deserialize, Serialize};

use crate::{Action, LogEntry};

/// Body von `POST /api/actions`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionRequest {
    pub action: Action,
}

/// Antwort auf eine angewendete Aktion.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionResponse {
    /// Revision des Stands nach der Aktion.
    pub revision: u64,
    /// Der Satz aus dem Protokoll (siehe `Applied::summary`).
    pub zusammenfassung: String,
    /// Zahl der neuen Protokolleinträge.
    pub protokolliert: usize,
}

/// Antwort auf `POST /api/sync` („jetzt abgleichen“).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SyncResponse {
    /// `false`, wenn keine Quelle eingerichtet ist (Jira oder GitHub mit Schlüssel).
    pub aktiv: bool,
    /// Revision des Stands nach dem Abgleich.
    pub revision: u64,
    /// Zahl der neuen Protokolleinträge.
    pub protokolliert: usize,
}

/// Ein Jira-Projekt für die Auswahl in der Inbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JiraProject {
    /// Schlüssel, z. B. `BAU`.
    pub key: String,
    /// Name, z. B. „Baukasten“.
    #[serde(default)]
    pub name: String,
}

/// Antwort auf `GET /api/jira/projekte`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraProjectsResponse {
    /// `false`, wenn Jira nicht eingerichtet ist oder der Schlüssel fehlt.
    pub aktiv: bool,
    #[serde(default)]
    pub projekte: Vec<JiraProject>,
}

/// Woran der Verbindungstest zu Jira gescheitert ist (oder `Ok`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JiraUrsache {
    /// Jira antwortet und kennt den Token.
    Ok,
    /// Adresse, E-Mail oder Token fehlen noch.
    #[default]
    Angaben,
    /// Unter der Adresse antwortet kein Jira (404 oder eine andere Seite).
    Adresse,
    /// Jira lehnt E-Mail oder Token ab (401).
    Token,
    /// Der Token ist gültig, hat aber nicht die nötigen Rechte (403).
    Berechtigung,
    /// Keine Verbindung (Netz, VPN, Adresse nicht auffindbar).
    Erreichbar,
    /// Etwas anderes (Kontingent, Störung bei Jira).
    Sonst,
}

/// Antwort auf `POST /api/jira/pruefen`: nur Lesen (`GET /rest/api/3/myself`), nie der Token.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraPruefung {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub ursache: JiraUrsache,
    /// Klartext für die Oberfläche.
    #[serde(default)]
    pub meldung: String,
    /// Bei Erfolg: der Anzeigename, unter dem Jira dich kennt.
    #[serde(default)]
    pub name: Option<String>,
}

/// Antwort auf `POST /api/oauth/{dienst}/widerrufen`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WiderrufAntwort {
    /// Die Tokens im Schlüsselbund sind gelöscht.
    pub getrennt: bool,
    /// Der Dienst selbst hat das Token widerrufen (nicht alle bieten das an).
    #[serde(default)]
    pub beim_dienst: bool,
    /// Was der Nutzer sonst noch tun kann oder warum der Dienst nicht widerrufen hat.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// Womit der Agent arbeitet: das Werkzeug, das im Container läuft. Umgesetzt ist bisher nur das
/// Claude Agent SDK; die anderen sind bekannte Werte, die in Klartext „noch nicht umgesetzt“ melden.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Anbieter {
    #[default]
    ClaudeAgentSdk,
    ClaudeCode,
    Codex,
}

impl Anbieter {
    pub const ALLE: [Anbieter; 3] = [
        Anbieter::ClaudeAgentSdk,
        Anbieter::ClaudeCode,
        Anbieter::Codex,
    ];

    /// Der Name in `config.toml` (`[agent] anbieter`) und im JSON.
    pub fn kennung(self) -> &'static str {
        match self {
            Anbieter::ClaudeAgentSdk => "claude-agent-sdk",
            Anbieter::ClaudeCode => "claude-code",
            Anbieter::Codex => "codex",
        }
    }

    pub fn aus_kennung(text: &str) -> Option<Anbieter> {
        let t = text.trim().to_lowercase();
        Self::ALLE.into_iter().find(|a| a.kennung() == t)
    }

    /// Der Name für die Oberfläche.
    pub fn bezeichnung(self) -> &'static str {
        match self {
            Anbieter::ClaudeAgentSdk => "Claude Agent SDK",
            Anbieter::ClaudeCode => "Claude Code",
            Anbieter::Codex => "Codex",
        }
    }

    /// Ob der Läufer dieses Werkzeug schon ausführen kann.
    pub fn umgesetzt(self) -> bool {
        self == Anbieter::ClaudeAgentSdk
    }

    /// Klartext für den Fall, dass ein Lauf dieses Werkzeug bräuchte, es aber fehlt.
    pub fn nicht_umgesetzt(self) -> Option<String> {
        (!self.umgesetzt()).then(|| {
            format!(
                "{} ist noch nicht umgesetzt. Wähle unter „Einrichten“ › „Modell“ das Claude Agent SDK.",
                self.bezeichnung()
            )
        })
    }
}

/// Die Anbindung des Agenten an sein Modell, wie `GET /api/health` sie meldet. Nie der Key selbst,
/// nur ob einer im Schlüsselbund liegt.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpunktStatus {
    /// `[agent.endpunkt]` (oder das ältere `[litellm]`) steht in `config.toml`.
    #[serde(default)]
    pub konfiguriert: bool,
    #[serde(default)]
    pub anbieter: Anbieter,
    /// Adresse des Endpunkts; leer = direkt beim Anbieter.
    #[serde(default)]
    pub url: Option<String>,
    /// Der Modellname.
    #[serde(default)]
    pub modell: Option<String>,
    /// Ein Key liegt im Schlüsselbund (`agent-key`, ersatzweise das ältere `litellm`).
    #[serde(default)]
    pub key: bool,
}

/// Die Container-Laufzeit, wie `GET /api/health` sie meldet.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerStatus {
    /// `docker` oder `podman`; `None`, wenn keine gefunden oder gewählt ist.
    #[serde(default)]
    pub laufzeit: Option<String>,
    /// `<cli> info` war erfolgreich: Die Engine läuft.
    #[serde(default)]
    pub engine_laeuft: bool,
    /// Woher die Wahl kommt: `konfiguration`, `umgebung`, `erkannt` oder `keine`.
    #[serde(default)]
    pub quelle: String,
    /// Was zu tun ist, wenn es nicht geht.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// Ob der echte Agent loslaufen kann, wie `GET /api/health` es meldet.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStatus {
    /// `echt` (Container) oder `simuliert` (Seed `demo`, nur zum Ausprobieren).
    #[serde(default)]
    pub modus: String,
    /// Läufe können starten: Alles Nötige ist eingerichtet (immer wahr im Modus `simuliert`).
    #[serde(default)]
    pub bereit: bool,
    /// Das Agent-Image ist gebaut (`image inspect`).
    #[serde(default)]
    pub image: bool,
    /// Name des Images, z. B. `localhost/cockpit-agent:1`.
    #[serde(default)]
    pub image_name: String,
    /// Das Repo, in dem der Agent arbeitet (`owner/name`), falls eindeutig.
    #[serde(default)]
    pub repo: Option<String>,
    /// Was noch fehlt, je Eintrag ein Satz in Klartext mit dem Ort im Assistenten.
    #[serde(default)]
    pub fehlt: Vec<String>,
}

/// Stand von „Image bauen“ (`GET`/`POST /api/agent/image`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageBau {
    /// Der Bau läuft gerade.
    #[serde(default)]
    pub laeuft: bool,
    /// Das Image gibt es (nach dem letzten Bau oder schon vorher).
    #[serde(default)]
    pub vorhanden: bool,
    /// Warum der letzte Bau scheiterte, in Klartext.
    #[serde(default)]
    pub fehler: Option<String>,
    /// Die letzten Zeilen der Ausgabe des Baus.
    #[serde(default)]
    pub zeilen: Vec<String>,
}

/// Antwort auf `GET /api/agent/pruefen/container`: Erreicht ein Container den Endpunkt?
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerPruefung {
    /// Es konnte geprüft werden (Image, Laufzeit und Endpunkt sind da).
    #[serde(default)]
    pub geprueft: bool,
    /// Der Mac (das Backend) erreicht den Endpunkt.
    #[serde(default)]
    pub mac_erreicht: bool,
    /// Der Container erreicht ihn.
    #[serde(default)]
    pub container_erreicht: bool,
    /// Der Endpunkt nimmt den Key im Container an.
    #[serde(default)]
    pub key_ok: bool,
    /// Was als Nächstes zu tun ist oder woran es lag.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// Antwort auf `GET /api/agent/pruefen`: was der Verbindungstest gefunden hat. Enthält nie den Key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpunktPruefung {
    /// Es gibt etwas zu prüfen (Adresse oder Modell eingetragen); sonst wurde nichts aufgerufen.
    #[serde(default)]
    pub konfiguriert: bool,
    /// Der Endpunkt hat auf `GET <url>/v1/models` geantwortet.
    #[serde(default)]
    pub erreichbar: bool,
    /// Der Endpunkt hat den Key angenommen.
    #[serde(default)]
    pub key_ok: bool,
    /// Der eingestellte Modellname steht in der Liste des Endpunkts.
    #[serde(default)]
    pub modell_vorhanden: bool,
    /// Was als Nächstes zu tun ist oder woran es lag.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// Antwort auf `GET /api/agent/modelle`: die Modell-IDs des Endpunkts (nur IDs, Claude-Modelle
/// zuerst, höchstens [`MAX_MODELLE`]). Enthält nie den Key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Modellliste {
    /// Die Liste konnte geladen werden.
    #[serde(default)]
    pub geladen: bool,
    #[serde(default)]
    pub erreichbar: bool,
    #[serde(default)]
    pub key_ok: bool,
    #[serde(default)]
    pub modelle: Vec<String>,
    /// Klartext, wenn etwas nicht ging.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// So viele Modell-IDs liefert die Liste höchstens.
pub const MAX_MODELLE: usize = 200;

/// Body von `PUT /api/schluessel/{name}`. Der Wert ist ein Geheimnis: Er steht nur im Eingabefeld,
/// geht einmal an den lokalen Server und kommt nie zurück. `Debug` zeigt ihn nicht.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SchluesselEingabe {
    pub wert: String,
}

impl std::fmt::Debug for SchluesselEingabe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SchluesselEingabe { wert: *** }")
    }
}

/// Ob ein Schlüssel im Schlüsselbund liegt – nie sein Wert.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SchluesselAntwort {
    pub name: String,
    #[serde(default)]
    pub gesetzt: bool,
}

/// Jira-Angaben für `config.toml`. `None` lässt den Wert unverändert, ein leerer Text entfernt ihn.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraEingabe {
    #[serde(default)]
    pub site: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub projects: Option<Vec<String>>,
}

/// Die Client-ID einer OAuth-App (nicht geheim). Leer entfernt den Abschnitt.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientEingabe {
    #[serde(default)]
    pub client_id: String,
}

/// Die GitHub-Repos für den Abgleich (`owner/name`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHubEingabe {
    #[serde(default)]
    pub repos: Vec<String>,
}

/// Anbieter, Adresse und Modell des Agenten. `None` lässt einen Wert unverändert; eine leere
/// Adresse heißt „direkt beim Anbieter“, ein leeres Modell entfernt den Namen.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpunktEingabe {
    #[serde(default)]
    pub anbieter: Option<Anbieter>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub modell: Option<String>,
}

/// Body von `PUT /api/konfiguration`: nur die Abschnitte, die sich ändern. Schlüssel gehören nicht
/// hierher, sondern zu `PUT /api/schluessel/{name}`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct KonfigAenderung {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira: Option<JiraEingabe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlassian: Option<ClientEingabe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_oauth: Option<ClientEingabe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GitHubEingabe>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "litellm")]
    pub endpunkt: Option<EndpunktEingabe>,
}

/// Antwort auf `PUT /api/konfiguration`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct KonfigAntwort {
    /// `config.toml` ist geschrieben.
    #[serde(default)]
    pub gespeichert: bool,
    /// Die Änderung gilt schon jetzt; sonst braucht es einen Neustart.
    #[serde(default)]
    pub wirkt_sofort: bool,
    /// Was der Nutzer wissen sollte.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// Antwort auf `GET /api/einrichtung`: die aktuellen, nicht geheimen Werte für den Assistenten.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Einrichtung {
    #[serde(default)]
    pub jira_site: String,
    #[serde(default)]
    pub jira_email: String,
    #[serde(default)]
    pub jira_projekte: Vec<String>,
    #[serde(default)]
    pub atlassian_client_id: Option<String>,
    #[serde(default)]
    pub github_client_id: Option<String>,
    #[serde(default)]
    pub github_repos: Vec<String>,
    #[serde(default)]
    pub anbieter: Anbieter,
    /// Adresse des Endpunkts; `None` = direkt beim Anbieter.
    #[serde(default, alias = "litellm_url")]
    pub endpunkt_url: Option<String>,
    #[serde(default, alias = "litellm_modell")]
    pub endpunkt_modell: Option<String>,
    /// Die Adresse, die in der GitHub App als Callback eingetragen wird.
    #[serde(default)]
    pub callback_github: String,
    /// Die Adresse, die in der Atlassian-App als Callback eingetragen wird.
    #[serde(default)]
    pub callback_atlassian: String,
    /// Welche Schlüssel im Schlüsselbund liegen.
    #[serde(default)]
    pub schluessel: Vec<SchluesselAntwort>,
}

impl Einrichtung {
    /// Liegt der Schlüssel `name` im Schlüsselbund?
    pub fn hat_schluessel(&self, name: &str) -> bool {
        let da = |n: &str| self.schluessel.iter().any(|s| s.name == n && s.gesetzt);
        // Der Key des Endpunkts hieß bis #31 `litellm` und gilt weiter.
        da(name) || (name == "agent-key" && da("litellm"))
    }
}

/// Fehlerantwort (4xx/5xx) mit deutscher Meldung für die Oberfläche.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ErrorBody {
    pub fehler: String,
}

/// Ein Ereignis auf `GET /api/events`.
///
/// Nach jeder angewendeten Aktion kommt eines mit den neuen Protokolleinträgen. Direkt
/// nach dem Verbinden kommt eines mit leerer Liste und der aktuellen Revision. Wer eine
/// Lücke in den Revisionen sieht (oder verbunden war und die Verbindung verlor), lädt
/// den Stand neu.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ServerEvent {
    pub revision: u64,
    #[serde(default)]
    pub eintraege: Vec<LogEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_has_the_documented_form() {
        let json = serde_json::to_string(&ActionRequest {
            action: Action::AdvanceAgents,
        })
        .unwrap();
        assert_eq!(json, r#"{"action":"AdvanceAgents"}"#);
        let back: ActionRequest =
            serde_json::from_str(r#"{"action":{"MarkDone":{"item":"bau142"}}}"#).unwrap();
        assert_eq!(
            back.action,
            Action::MarkDone {
                item: "bau142".into()
            }
        );
    }

    #[test]
    fn event_without_entries_is_readable() {
        let e: ServerEvent = serde_json::from_str(r#"{"revision":3}"#).unwrap();
        assert_eq!(e.revision, 3);
        assert!(e.eintraege.is_empty());
    }

    #[test]
    fn setup_statuses_read_old_and_partial_json() {
        assert_eq!(
            serde_json::from_str::<EndpunktStatus>("{}").unwrap(),
            EndpunktStatus::default()
        );
        assert_eq!(
            serde_json::from_str::<ContainerStatus>("{}").unwrap(),
            ContainerStatus::default()
        );
        assert_eq!(
            serde_json::from_str::<Einrichtung>("{}").unwrap(),
            Einrichtung::default()
        );
        let p: EndpunktPruefung =
            serde_json::from_str(r#"{"konfiguriert":true,"erreichbar":true}"#).unwrap();
        assert!(p.erreichbar && !p.key_ok && !p.modell_vorhanden);
    }

    #[test]
    fn key_input_never_shows_its_value() {
        let e = SchluesselEingabe {
            wert: "test-wert-123".into(),
        };
        assert!(!format!("{e:?}").contains("test-wert-123"));
    }

    #[test]
    fn config_change_only_sends_touched_sections() {
        let a = KonfigAenderung {
            github: Some(GitHubEingabe {
                repos: vec!["firma/app".into()],
            }),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"github":{"repos":["firma/app"]}}"#
        );
        let back: KonfigAenderung = serde_json::from_str("{}").unwrap();
        assert_eq!(back, KonfigAenderung::default());
    }

    #[test]
    fn providers_have_stable_names_and_only_the_sdk_is_implemented() {
        for a in Anbieter::ALLE {
            assert_eq!(Anbieter::aus_kennung(a.kennung()), Some(a));
            let j = serde_json::to_string(&a).unwrap();
            assert_eq!(j, format!("\"{}\"", a.kennung()));
        }
        assert_eq!(Anbieter::default(), Anbieter::ClaudeAgentSdk);
        assert_eq!(Anbieter::aus_kennung(" Codex "), Some(Anbieter::Codex));
        assert_eq!(Anbieter::aus_kennung("gpt"), None);
        assert!(Anbieter::ClaudeAgentSdk.umgesetzt());
        assert!(Anbieter::ClaudeAgentSdk.nicht_umgesetzt().is_none());
        for a in [Anbieter::ClaudeCode, Anbieter::Codex] {
            assert!(
                a.nicht_umgesetzt()
                    .unwrap()
                    .contains("noch nicht umgesetzt")
            );
        }
    }

    #[test]
    fn the_old_key_name_counts_as_the_agent_key() {
        let mit = |n: &str| Einrichtung {
            schluessel: vec![SchluesselAntwort {
                name: n.into(),
                gesetzt: true,
            }],
            ..Default::default()
        };
        assert!(mit("agent-key").hat_schluessel("agent-key"));
        assert!(mit("litellm").hat_schluessel("agent-key"));
        assert!(!mit("agent-key").hat_schluessel("litellm"));
        assert!(!Einrichtung::default().hat_schluessel("agent-key"));
    }

    #[test]
    fn old_setup_names_still_read() {
        let e: Einrichtung =
            serde_json::from_str(r#"{"litellm_url":"http://x:1","litellm_modell":"m"}"#).unwrap();
        assert_eq!(e.endpunkt_url.as_deref(), Some("http://x:1"));
        assert_eq!(e.endpunkt_modell.as_deref(), Some("m"));
        let k: KonfigAenderung =
            serde_json::from_str(r#"{"litellm":{"url":"http://x:1"}}"#).unwrap();
        assert_eq!(k.endpunkt.unwrap().url.as_deref(), Some("http://x:1"));
    }
}
