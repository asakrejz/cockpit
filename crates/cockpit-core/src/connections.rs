//! Verbindungen zu Jira, GitHub und dem Agenten, wie das Backend sie meldet.
//!
//! [`Connections`] hält nur Haken – **nie Schlüssel**. Ob ein Schlüssel wirklich gilt, kann nur
//! der Dienst sagen; das Backend prüft es, der Browser speichert nichts davon.

use serde::{Deserialize, Serialize};

/// Womit sich das Backend bei einem Dienst anmeldet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    /// Nicht verbunden.
    #[default]
    None,
    /// API-Token oder Personal Access Token (`cockpit-server token setzen`).
    Token,
    /// Anmeldung im Browser per OAuth.
    OAuth,
}

impl AuthKind {
    /// Kurzer Text für die Oberfläche.
    pub fn label(self) -> &'static str {
        match self {
            AuthKind::None => "Getrennt",
            AuthKind::Token => "Token",
            AuthKind::OAuth => "OAuth",
        }
    }
}

/// Verbindungsstatus eines Dienstes, wie ihn das Backend in `/api/health` meldet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    #[serde(default)]
    pub verbunden: bool,
    /// Art der Anmeldung; `none`, solange getrennt.
    #[serde(default)]
    pub art: AuthKind,
    /// Die OAuth-App ist in `config.toml` eingerichtet und ihr Secret liegt im Schlüsselbund:
    /// „Mit … anmelden“ ist möglich.
    #[serde(default)]
    pub oauth_moeglich: bool,
    /// Bei Atlassian: die gewählte Site.
    #[serde(default)]
    pub site: Option<String>,
}

/// Welche Verbindungen stehen und was der Agent selbstständig darf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connections {
    pub jira: bool,
    pub github: bool,
    /// Art der Anmeldung (nur vom Server gemeldet, wird nicht gespeichert).
    pub jira_auth: AuthKind,
    pub github_auth: AuthKind,
    pub agent: bool,
    pub may_branch: bool,
    pub may_open_pr: bool,
    pub may_run_tests: bool,
}

impl Default for Connections {
    fn default() -> Self {
        Self {
            jira: false,
            github: false,
            jira_auth: AuthKind::None,
            github_auth: AuthKind::None,
            agent: false,
            may_branch: true,
            may_open_pr: true,
            may_run_tests: true,
        }
    }
}

impl Connections {
    /// Übernimmt den Stand, den das Backend meldet (Haken und Art der Anmeldung).
    pub fn apply_services(&mut self, github: &ServiceStatus, jira: &ServiceStatus) {
        self.github = github.verbunden;
        self.github_auth = github.art;
        self.jira = jira.verbunden;
        self.jira_auth = jira.art;
    }

    /// Warum der Agent kein Ticket umsetzen darf – oder `None`, wenn er darf.
    pub fn agent_blocked(&self) -> Option<&'static str> {
        if !self.agent {
            Some("Der Agent ist nicht verbunden.")
        } else if !self.may_branch {
            Some("Der Agent darf keine Branches erstellen.")
        } else if !self.may_open_pr {
            Some("Der Agent darf keine Pull Requests erstellen.")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_reports_the_kind() {
        let mut c = Connections::default();
        let oauth = ServiceStatus {
            verbunden: true,
            art: AuthKind::OAuth,
            ..Default::default()
        };
        c.apply_services(&oauth, &ServiceStatus::default());
        assert!(c.github && !c.jira);
        assert_eq!(
            (c.github_auth, c.jira_auth),
            (AuthKind::OAuth, AuthKind::None)
        );
    }

    #[test]
    fn service_status_reads_old_and_partial_json() {
        let s: ServiceStatus = serde_json::from_str("{}").unwrap();
        assert_eq!(s, ServiceStatus::default());
        let s: ServiceStatus =
            serde_json::from_str(r#"{"verbunden":true,"art":"oauth","site":"firma"}"#).unwrap();
        assert_eq!(s.art, AuthKind::OAuth);
        assert_eq!(
            serde_json::to_string(&AuthKind::Token).unwrap(),
            "\"token\""
        );
    }

    #[test]
    fn agent_needs_connection_and_permissions() {
        let mut c = Connections::default();
        assert!(c.agent_blocked().is_some());
        c.agent = true;
        assert_eq!(c.agent_blocked(), None);
        c.may_open_pr = false;
        assert!(c.agent_blocked().unwrap().contains("Pull Requests"));
        c.may_branch = false;
        assert!(c.agent_blocked().unwrap().contains("Branches"));
        c.may_branch = true;
        c.may_open_pr = true;
        c.may_run_tests = false;
        assert_eq!(c.agent_blocked(), None, "Tests sind optional");
    }
}
