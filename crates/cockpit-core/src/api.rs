//! Die JSON-Formen zwischen App und Backend (Server-Modus).
//!
//! * `GET /api/workspace`: der Stand in der Form von [`persist`](crate::persist), dazu `revision`
//! * `POST /api/actions`: Body [`ActionRequest`], Antwort [`ActionResponse`] oder [`ErrorBody`]
//! * `GET /api/events`: Server-Sent Events, jedes Ereignis trägt ein [`ServerEvent`]
//! * `GET /api/oauth/{github|atlassian}/start` und `…/callback`: Anmeldung per OAuth (Navigation des
//!   Browsers, leitet nach `/` um); `POST …/widerrufen`: [`WiderrufAntwort`]
//! * `GET /api/jira/projekte`: die Jira-Projekte für die Auswahl, [`JiraProjectsResponse`]
//! * `GET /api/litellm/pruefen`: Verbindungstest zum LiteLLM-Proxy, [`LitellmPruefung`]
//! * `GET /api/einrichtung`: was der Einrichtungsassistent vorbelegt, [`Einrichtung`]
//! * `PUT /api/konfiguration`: Body [`KonfigAenderung`], schreibt `config.toml`, Antwort [`KonfigAntwort`]
//! * `PUT /api/schluessel/{name}`: Body [`SchluesselEingabe`], legt den Schlüssel im Schlüsselbund
//!   ab, Antwort [`SchluesselAntwort`] (nur „gesetzt“, nie der Wert)
//! * `GET /api/agent/image` / `POST /api/agent/image`: Stand bzw. Start von „Image bauen“, [`ImageBau`]
//! * `GET /api/litellm/pruefen/container`: dieselbe Prüfung aus einem Container, [`ContainerPruefung`]
//! * `GET /api/health`: unter `agent` ein [`AgentStatus`], unter `litellm` ein [`LitellmStatus`], unter `container` ein [`ContainerStatus`]
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

/// Die Anbindung an den LiteLLM-Proxy, wie `GET /api/health` sie meldet. Nie der Key selbst,
/// nur ob einer im Schlüsselbund liegt.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LitellmStatus {
    /// `[litellm] url` steht in `config.toml`.
    #[serde(default)]
    pub konfiguriert: bool,
    #[serde(default)]
    pub url: Option<String>,
    /// Der Modellname aus `[litellm] modell`.
    #[serde(default)]
    pub modell: Option<String>,
    /// Ein Key liegt im Schlüsselbund (`cockpit-server token setzen litellm`).
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

/// Antwort auf `GET /api/litellm/pruefen/container`: Erreicht ein Container den Proxy?
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContainerPruefung {
    /// Es konnte geprüft werden (Image, Laufzeit und Proxy sind da).
    #[serde(default)]
    pub geprueft: bool,
    /// Der Mac (das Backend) erreicht den Proxy.
    #[serde(default)]
    pub mac_erreicht: bool,
    /// Der Container erreicht ihn.
    #[serde(default)]
    pub container_erreicht: bool,
    /// Der Proxy nimmt den Key im Container an.
    #[serde(default)]
    pub key_ok: bool,
    /// Was als Nächstes zu tun ist oder woran es lag.
    #[serde(default)]
    pub hinweis: Option<String>,
}

/// Antwort auf `GET /api/litellm/pruefen`: was der Verbindungstest gefunden hat. Enthält nie den Key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LitellmPruefung {
    /// `[litellm] url` ist eingerichtet; sonst wurde nichts geprüft.
    #[serde(default)]
    pub konfiguriert: bool,
    /// Der Proxy hat auf `GET <url>/v1/models` geantwortet.
    #[serde(default)]
    pub erreichbar: bool,
    /// Der Proxy hat den Key angenommen.
    #[serde(default)]
    pub key_ok: bool,
    /// Der konfigurierte Modellname steht in der Liste des Proxys.
    #[serde(default)]
    pub modell_vorhanden: bool,
    /// Was als Nächstes zu tun ist oder woran es lag.
    #[serde(default)]
    pub hinweis: Option<String>,
}

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

/// Adresse und Modell des LiteLLM-Proxys. Eine leere Adresse entfernt den Abschnitt.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LitellmEingabe {
    #[serde(default)]
    pub url: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub litellm: Option<LitellmEingabe>,
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
    pub litellm_url: Option<String>,
    #[serde(default)]
    pub litellm_modell: Option<String>,
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
        self.schluessel.iter().any(|s| s.name == name && s.gesetzt)
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
            serde_json::from_str::<LitellmStatus>("{}").unwrap(),
            LitellmStatus::default()
        );
        assert_eq!(
            serde_json::from_str::<ContainerStatus>("{}").unwrap(),
            ContainerStatus::default()
        );
        assert_eq!(
            serde_json::from_str::<Einrichtung>("{}").unwrap(),
            Einrichtung::default()
        );
        let p: LitellmPruefung =
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
}
