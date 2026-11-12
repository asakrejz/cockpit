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
    #[serde(rename = "zusammenfassung")]
    pub summary: String,
    /// Zahl der neuen Protokolleinträge.
    #[serde(rename = "protokolliert")]
    pub logged: usize,
}

/// Antwort auf `POST /api/sync` („jetzt abgleichen“).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SyncResponse {
    /// `false`, wenn keine Quelle eingerichtet ist (Jira oder GitHub mit Schlüssel).
    #[serde(rename = "aktiv")]
    pub active: bool,
    /// Revision des Stands nach dem Abgleich.
    pub revision: u64,
    /// Zahl der neuen Protokolleinträge.
    #[serde(rename = "protokolliert")]
    pub logged: usize,
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
    #[serde(rename = "aktiv")]
    pub active: bool,
    #[serde(default)]
    #[serde(rename = "projekte")]
    pub projects: Vec<JiraProject>,
}

/// Woran der Verbindungstest zu Jira gescheitert ist (oder `Ok`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JiraCause {
    /// Jira antwortet und kennt den Token.
    Ok,
    /// Adresse, E-Mail oder Token fehlen noch.
    #[default]
    #[serde(rename = "angaben")]
    Details,
    /// Unter der Adresse antwortet kein Jira (404 oder eine andere Seite).
    #[serde(rename = "adresse")]
    Address,
    /// Jira lehnt E-Mail oder Token ab (401).
    Token,
    /// Der Token ist gültig, hat aber nicht die nötigen Rechte (403).
    #[serde(rename = "berechtigung")]
    Permission,
    /// Keine Verbindung (Netz, VPN, Adresse nicht auffindbar).
    #[serde(rename = "erreichbar")]
    Reachable,
    /// Etwas anderes (Kontingent, Störung bei Jira).
    #[serde(rename = "sonst")]
    Other,
}

/// Antwort auf `POST /api/jira/pruefen`: nur Lesen (`GET /rest/api/3/myself`), nie der Token.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraCheck {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    #[serde(rename = "ursache")]
    pub cause: JiraCause,
    /// Klartext für die Oberfläche.
    #[serde(default)]
    #[serde(rename = "meldung")]
    pub message: String,
    /// Bei Erfolg: der Anzeigename, unter dem Jira dich kennt.
    #[serde(default)]
    pub name: Option<String>,
}

/// Antwort auf `POST /api/oauth/{dienst}/widerrufen`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RevokeResponse {
    /// Die Tokens im Schlüsselbund sind gelöscht.
    #[serde(rename = "getrennt")]
    pub disconnected: bool,
    /// Der Dienst selbst hat das Token widerrufen (nicht alle bieten das an).
    #[serde(default)]
    #[serde(rename = "beim_dienst")]
    pub at_service: bool,
    /// Was der Nutzer sonst noch tun kann oder warum der Dienst nicht widerrufen hat.
    #[serde(default)]
    #[serde(rename = "hinweis")]
    pub hint: Option<String>,
}

/// Womit der Agent arbeitet: das Werkzeug, das im Container läuft. Umgesetzt ist bisher nur das
/// Claude Agent SDK; die anderen sind bekannte Werte, die in Klartext „noch nicht umgesetzt“ melden.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[default]
    ClaudeAgentSdk,
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 3] = [
        Provider::ClaudeAgentSdk,
        Provider::ClaudeCode,
        Provider::Codex,
    ];

    /// Der Name in `config.toml` (`[agent] anbieter`) und im JSON.
    pub fn id(self) -> &'static str {
        match self {
            Provider::ClaudeAgentSdk => "claude-agent-sdk",
            Provider::ClaudeCode => "claude-code",
            Provider::Codex => "codex",
        }
    }

    pub fn from_id(text: &str) -> Option<Provider> {
        let t = text.trim().to_lowercase();
        Self::ALL.into_iter().find(|a| a.id() == t)
    }

    /// Der Name für die Oberfläche.
    pub fn label(self) -> &'static str {
        match self {
            Provider::ClaudeAgentSdk => "Claude Agent SDK",
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    /// Ob der Läufer dieses Werkzeug schon ausführen kann.
    pub fn implemented(self) -> bool {
        self == Provider::ClaudeAgentSdk
    }

    /// Klartext für den Fall, dass ein Lauf dieses Werkzeug bräuchte, es aber fehlt.
    pub fn not_implemented(self) -> Option<String> {
        (!self.implemented()).then(|| {
            format!(
                "{} ist noch nicht umgesetzt. Wähle unter „Einrichten“ › „Modell“ das Claude Agent SDK.",
                self.label()
            )
        })
    }
}

/// Die Anbindung des Agenten an sein Modell, wie `GET /api/health` sie meldet. Nie der Key selbst,
/// nur ob einer im Schlüsselbund liegt.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointStatus {
    /// `[agent.endpunkt]` (oder das ältere `[litellm]`) steht in `config.toml`.
    #[serde(default)]
    #[serde(rename = "konfiguriert")]
    pub configured: bool,
    #[serde(default)]
    #[serde(rename = "anbieter")]
    pub provider: Provider,
    /// Adresse des Endpunkts; leer = direkt beim Anbieter.
    #[serde(default)]
    pub url: Option<String>,
    /// Der Modellname.
    #[serde(default)]
    #[serde(rename = "modell")]
    pub model: Option<String>,
    /// Ein Key liegt im Schlüsselbund (`agent-key`, ersatzweise das ältere `litellm`).
    #[serde(default)]
    pub key: bool,
}

/// Die Container-Laufzeit, wie `GET /api/health` sie meldet.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerStatus {
    /// `docker` oder `podman`; `None`, wenn keine gefunden oder gewählt ist.
    #[serde(default)]
    #[serde(rename = "laufzeit")]
    pub runtime: Option<String>,
    /// `<cli> info` war erfolgreich: Die Engine läuft.
    #[serde(default)]
    #[serde(rename = "engine_laeuft")]
    pub engine_running: bool,
    /// Woher die Wahl kommt: `konfiguration`, `umgebung`, `erkannt` oder `keine`.
    #[serde(default)]
    #[serde(rename = "quelle")]
    pub source: String,
    /// Was zu tun ist, wenn es nicht geht.
    #[serde(default)]
    #[serde(rename = "hinweis")]
    pub hint: Option<String>,
}

/// Ob der echte Agent loslaufen kann, wie `GET /api/health` es meldet.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStatus {
    /// `echt` (Container) oder `simuliert` (Seed `demo`, nur zum Ausprobieren).
    #[serde(default)]
    #[serde(rename = "modus")]
    pub mode: String,
    /// Läufe können starten: Alles Nötige ist eingerichtet (immer wahr im Modus `simuliert`).
    #[serde(default)]
    #[serde(rename = "bereit")]
    pub ready: bool,
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
    #[serde(rename = "fehlt")]
    pub missing: Vec<String>,
}

/// Stand von „Image bauen“ (`GET`/`POST /api/agent/image`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageBau {
    /// Der Bau läuft gerade.
    #[serde(default)]
    #[serde(rename = "laeuft")]
    pub running: bool,
    /// Das Image gibt es (nach dem letzten Bau oder schon vorher).
    #[serde(default)]
    #[serde(rename = "vorhanden")]
    pub present: bool,
    /// Warum der letzte Bau scheiterte, in Klartext.
    #[serde(default)]
    #[serde(rename = "fehler")]
    pub error: Option<String>,
    /// Die letzten Zeilen der Ausgabe des Baus.
    #[serde(default)]
    #[serde(rename = "zeilen")]
    pub lines: Vec<String>,
}

/// Antwort auf `GET /api/agent/pruefen/container`: Erreicht ein Container den Endpunkt?
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerCheck {
    /// Es konnte geprüft werden (Image, Laufzeit und Endpunkt sind da).
    #[serde(default)]
    #[serde(rename = "geprueft")]
    pub checked: bool,
    /// Der Mac (das Backend) erreicht den Endpunkt.
    #[serde(default)]
    #[serde(rename = "mac_erreicht")]
    pub mac_reached: bool,
    /// Der Container erreicht ihn.
    #[serde(default)]
    #[serde(rename = "container_erreicht")]
    pub container_reached: bool,
    /// Der Endpunkt nimmt den Key im Container an.
    #[serde(default)]
    pub key_ok: bool,
    /// Was als Nächstes zu tun ist oder woran es lag.
    #[serde(default)]
    #[serde(rename = "hinweis")]
    pub hint: Option<String>,
}

/// Antwort auf `GET /api/agent/pruefen`: was der Verbindungstest gefunden hat. Enthält nie den Key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointCheck {
    /// Es gibt etwas zu prüfen (Adresse oder Modell eingetragen); sonst wurde nichts aufgerufen.
    #[serde(default)]
    #[serde(rename = "konfiguriert")]
    pub configured: bool,
    /// Der Endpunkt hat auf `GET <url>/v1/models` geantwortet.
    #[serde(default)]
    #[serde(rename = "erreichbar")]
    pub reachable: bool,
    /// Der Endpunkt hat den Key angenommen.
    #[serde(default)]
    pub key_ok: bool,
    /// Der eingestellte Modellname steht in der Liste des Endpunkts.
    #[serde(default)]
    #[serde(rename = "modell_vorhanden")]
    pub model_present: bool,
    /// Was als Nächstes zu tun ist oder woran es lag.
    #[serde(default)]
    #[serde(rename = "hinweis")]
    pub hint: Option<String>,
}

/// Antwort auf `GET /api/agent/modelle`: die Modell-IDs des Endpunkts (nur IDs, Claude-Modelle
/// zuerst, höchstens [`MAX_MODELLE`]). Enthält nie den Key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelList {
    /// Die Liste konnte geladen werden.
    #[serde(default)]
    #[serde(rename = "geladen")]
    pub loaded: bool,
    #[serde(default)]
    #[serde(rename = "erreichbar")]
    pub reachable: bool,
    #[serde(default)]
    pub key_ok: bool,
    #[serde(default)]
    #[serde(rename = "modelle")]
    pub models: Vec<String>,
    /// Klartext, wenn etwas nicht ging.
    #[serde(default)]
    #[serde(rename = "hinweis")]
    pub hint: Option<String>,
}

/// So viele Modell-IDs liefert die Liste höchstens.
pub const MAX_MODELS: usize = 200;

/// Body von `PUT /api/schluessel/{name}`. Der Wert ist ein Geheimnis: Er steht nur im Eingabefeld,
/// geht einmal an den lokalen Server und kommt nie zurück. `Debug` zeigt ihn nicht.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct KeyInput {
    #[serde(rename = "wert")]
    pub value: String,
}

impl std::fmt::Debug for KeyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SchluesselEingabe { wert: *** }")
    }
}

/// Ob ein Schlüssel im Schlüsselbund liegt – nie sein Wert.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KeyResponse {
    pub name: String,
    #[serde(default)]
    #[serde(rename = "gesetzt")]
    pub set: bool,
}

/// Jira-Angaben für `config.toml`. `None` lässt den Wert unverändert, ein leerer Text entfernt ihn.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraInput {
    #[serde(default)]
    pub site: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub projects: Option<Vec<String>>,
}

/// Die Client-ID einer OAuth-App (nicht geheim). Leer entfernt den Abschnitt.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientInput {
    #[serde(default)]
    pub client_id: String,
}

/// Die GitHub-Repos für den Abgleich (`owner/name`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHubInput {
    #[serde(default)]
    pub repos: Vec<String>,
}

/// Anbieter, Adresse und Modell des Agenten. `None` lässt einen Wert unverändert; eine leere
/// Adresse heißt „direkt beim Anbieter“, ein leeres Modell entfernt den Namen.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointInput {
    #[serde(default)]
    #[serde(rename = "anbieter")]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    #[serde(rename = "modell")]
    pub model: Option<String>,
}

/// Body von `PUT /api/konfiguration`: nur die Abschnitte, die sich ändern. Schlüssel gehören nicht
/// hierher, sondern zu `PUT /api/schluessel/{name}`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira: Option<JiraInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlassian: Option<ClientInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_oauth: Option<ClientInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GitHubInput>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "litellm")]
    #[serde(rename = "endpunkt")]
    pub endpoint: Option<EndpointInput>,
}

/// Antwort auf `PUT /api/konfiguration`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigResponse {
    /// `config.toml` ist geschrieben.
    #[serde(default)]
    #[serde(rename = "gespeichert")]
    pub stored: bool,
    /// Die Änderung gilt schon jetzt; sonst braucht es einen Neustart.
    #[serde(default)]
    #[serde(rename = "wirkt_sofort")]
    pub takes_effect_immediately: bool,
    /// Was der Nutzer wissen sollte.
    #[serde(default)]
    #[serde(rename = "hinweis")]
    pub hint: Option<String>,
}

/// Antwort auf `GET /api/einrichtung`: die aktuellen, nicht geheimen Werte für den Assistenten.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    #[serde(default)]
    pub jira_site: String,
    #[serde(default)]
    pub jira_email: String,
    #[serde(default)]
    #[serde(rename = "jira_projekte")]
    pub jira_projects: Vec<String>,
    #[serde(default)]
    pub atlassian_client_id: Option<String>,
    #[serde(default)]
    pub github_client_id: Option<String>,
    #[serde(default)]
    pub github_repos: Vec<String>,
    #[serde(default)]
    #[serde(rename = "anbieter")]
    pub provider: Provider,
    /// Adresse des Endpunkts; `None` = direkt beim Anbieter.
    #[serde(default, alias = "litellm_url")]
    #[serde(rename = "endpunkt_url")]
    pub endpoint_url: Option<String>,
    #[serde(default, alias = "litellm_modell")]
    #[serde(rename = "endpunkt_modell")]
    pub endpoint_model: Option<String>,
    /// Die Adresse, die in der GitHub App als Callback eingetragen wird.
    #[serde(default)]
    pub callback_github: String,
    /// Die Adresse, die in der Atlassian-App als Callback eingetragen wird.
    #[serde(default)]
    pub callback_atlassian: String,
    /// Welche Schlüssel im Schlüsselbund liegen.
    #[serde(default)]
    #[serde(rename = "schluessel")]
    pub key: Vec<KeyResponse>,
}

impl Setup {
    /// Liegt der Schlüssel `name` im Schlüsselbund?
    pub fn has_key(&self, name: &str) -> bool {
        let da = |n: &str| self.key.iter().any(|s| s.name == n && s.set);
        // Der Key des Endpunkts hieß bis #31 `litellm` und gilt weiter.
        da(name) || (name == "agent-key" && da("litellm"))
    }
}

/// Fehlerantwort (4xx/5xx) mit deutscher Meldung für die Oberfläche.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ErrorBody {
    #[serde(rename = "fehler")]
    pub error: String,
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
    #[serde(rename = "eintraege")]
    pub entries: Vec<LogEntry>,
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
        assert!(e.entries.is_empty());
    }

    #[test]
    fn setup_statuses_read_old_and_partial_json() {
        assert_eq!(
            serde_json::from_str::<EndpointStatus>("{}").unwrap(),
            EndpointStatus::default()
        );
        assert_eq!(
            serde_json::from_str::<ContainerStatus>("{}").unwrap(),
            ContainerStatus::default()
        );
        assert_eq!(
            serde_json::from_str::<Setup>("{}").unwrap(),
            Setup::default()
        );
        let p: EndpointCheck =
            serde_json::from_str(r#"{"konfiguriert":true,"erreichbar":true}"#).unwrap();
        assert!(p.reachable && !p.key_ok && !p.model_present);
    }

    #[test]
    fn key_input_never_shows_its_value() {
        let e = KeyInput {
            value: "test-wert-123".into(),
        };
        assert!(!format!("{e:?}").contains("test-wert-123"));
    }

    #[test]
    fn config_change_only_sends_touched_sections() {
        let a = ConfigChange {
            github: Some(GitHubInput {
                repos: vec!["firma/app".into()],
            }),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"github":{"repos":["firma/app"]}}"#
        );
        let back: ConfigChange = serde_json::from_str("{}").unwrap();
        assert_eq!(back, ConfigChange::default());
    }

    #[test]
    fn providers_have_stable_names_and_only_the_sdk_is_implemented() {
        for a in Provider::ALL {
            assert_eq!(Provider::from_id(a.id()), Some(a));
            let j = serde_json::to_string(&a).unwrap();
            assert_eq!(j, format!("\"{}\"", a.id()));
        }
        assert_eq!(Provider::default(), Provider::ClaudeAgentSdk);
        assert_eq!(Provider::from_id(" Codex "), Some(Provider::Codex));
        assert_eq!(Provider::from_id("gpt"), None);
        assert!(Provider::ClaudeAgentSdk.implemented());
        assert!(Provider::ClaudeAgentSdk.not_implemented().is_none());
        for a in [Provider::ClaudeCode, Provider::Codex] {
            assert!(
                a.not_implemented()
                    .unwrap()
                    .contains("noch nicht umgesetzt")
            );
        }
    }

    #[test]
    fn the_old_key_name_counts_as_the_agent_key() {
        let with = |n: &str| Setup {
            key: vec![KeyResponse {
                name: n.into(),
                set: true,
            }],
            ..Default::default()
        };
        assert!(with("agent-key").has_key("agent-key"));
        assert!(with("litellm").has_key("agent-key"));
        assert!(!with("agent-key").has_key("litellm"));
        assert!(!Setup::default().has_key("agent-key"));
    }

    #[test]
    fn old_setup_names_still_read() {
        let e: Setup =
            serde_json::from_str(r#"{"litellm_url":"http://x:1","litellm_modell":"m"}"#).unwrap();
        assert_eq!(e.endpoint_url.as_deref(), Some("http://x:1"));
        assert_eq!(e.endpoint_model.as_deref(), Some("m"));
        let k: ConfigChange =
            serde_json::from_str(r#"{"litellm":{"url":"http://x:1"}}"#).unwrap();
        assert_eq!(k.endpoint.unwrap().url.as_deref(), Some("http://x:1"));
    }
}
