//! Die JSON-Formen zwischen App und Backend (Server-Modus).
//!
//! * `GET /api/workspace`: der Stand in der Form von [`persist`](crate::persist), dazu `revision`
//! * `POST /api/actions`: Body [`ActionRequest`], Antwort [`ActionResponse`] oder [`ErrorBody`]
//! * `GET /api/events`: Server-Sent Events, jedes Ereignis trägt ein [`ServerEvent`]
//! * `GET /api/oauth/{github|atlassian}/start` und `…/callback`: Anmeldung per OAuth (Navigation des
//!   Browsers, leitet nach `/` um); `POST …/widerrufen`: [`WiderrufAntwort`]
//! * `GET /api/jira/projekte`: die Jira-Projekte für die Auswahl, [`JiraProjectsResponse`]
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
}
