//! Connections to Jira, GitHub and the agent, as the backend reports them.
//!
//! [`Connections`] holds only checkmarks – **never keys**. Whether a key really works, only
//! the service can say; the backend checks it, the browser stores nothing of it.

use serde::{Deserialize, Serialize};

/// How the backend signs in at a service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    /// Not connected.
    #[default]
    None,
    /// API token or personal access token (entered in the setup assistant).
    Token,
    /// Sign-in in the browser via OAuth.
    OAuth,
}

impl AuthKind {
    /// Short text for the UI.
    pub fn label(self) -> &'static str {
        match self {
            AuthKind::None => "Getrennt",
            AuthKind::Token => "Token",
            AuthKind::OAuth => "OAuth",
        }
    }
}

/// Connection status of a service, as the backend reports it in `/api/health`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    #[serde(default)]
    pub connected: bool,
    /// Kind of sign-in; `none` while disconnected.
    #[serde(default)]
    pub kind: AuthKind,
    /// The OAuth app is set up in `config.toml` and its secret is in the .env file:
    /// „Mit … anmelden“ is possible.
    #[serde(default)]
    pub oauth_possible: bool,
    /// For Atlassian: the chosen site.
    #[serde(default)]
    pub site: Option<String>,
}

/// Which connections stand and what the agent may do on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connections {
    pub jira: bool,
    pub github: bool,
    /// Kind of sign-in (only reported by the server, not stored).
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
    /// Takes over the state the backend reports (checkmarks and kind of sign-in).
    pub fn apply_services(&mut self, github: &ServiceStatus, jira: &ServiceStatus) {
        self.github = github.connected;
        self.github_auth = github.kind;
        self.jira = jira.connected;
        self.jira_auth = jira.kind;
    }

    /// Why the agent may not implement a ticket – or `None` if it may.
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
mod tests;
