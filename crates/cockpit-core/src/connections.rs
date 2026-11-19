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
    pub fn label(self) -> cockpit_i18n::Msg {
        match self {
            AuthKind::None => cockpit_i18n::msg!(CoreMiscAuthNone),
            AuthKind::Token => cockpit_i18n::msg!(CoreMiscAuthToken),
            AuthKind::OAuth => cockpit_i18n::msg!(CoreMiscAuthOauth),
        }
    }
}

/// The sign-in method the user picked for a service (`auth = "token" | "oauth"` in the
/// `[github]` / `[jira]` section). Without a stored choice the older rule stays: a working OAuth
/// sign-in first, then the token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthChoice {
    /// Use the API token or personal access token, whatever else is stored.
    Token,
    /// Use the sign-in via OAuth, whatever else is stored.
    OAuth,
}

impl AuthChoice {
    /// The text in `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            AuthChoice::Token => "token",
            AuthChoice::OAuth => "oauth",
        }
    }

    /// **The one rule** for which credentials a service uses. Credential selection (sync,
    /// agent, repo list) and the connection status all call this.
    ///
    /// * no choice: a working sign-in goes before the token, an expired one falls back to it;
    /// * `token`: only the token counts, a stored sign-in is kept but not used;
    /// * `oauth`: only a working sign-in counts, a stored token is kept but not used.
    pub fn pick(choice: Option<Self>, token: bool, oauth: OAuthTokens) -> AuthKind {
        match (choice, token, oauth) {
            (None | Some(AuthChoice::OAuth), _, OAuthTokens::Valid) => AuthKind::OAuth,
            (None | Some(AuthChoice::Token), true, _) => AuthKind::Token,
            _ => AuthKind::None,
        }
    }
}

impl From<AuthChoice> for AuthKind {
    fn from(c: AuthChoice) -> Self {
        match c {
            AuthChoice::Token => AuthKind::Token,
            AuthChoice::OAuth => AuthKind::OAuth,
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
    /// The sign-in via OAuth has run out and cannot be renewed: sign in again.
    /// A token that is stored as a fallback still counts as connected.
    #[serde(default)]
    pub expired: bool,
    /// The stored choice of the sign-in method, `None` if the user never picked one.
    #[serde(default)]
    pub choice: Option<AuthChoice>,
    /// A sign-in via OAuth is stored (working or not), even if it is not the one in use.
    #[serde(default)]
    pub oauth_stored: bool,
}

/// What the key store holds for the OAuth sign-in of one service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OAuthTokens {
    /// No sign-in stored.
    Missing,
    /// A sign-in that works (or can be renewed with the refresh token).
    Valid,
    /// The access has run out and there is no refresh token.
    Expired,
}

impl ServiceStatus {
    /// **The one place** that decides whether a service counts as connected. Summary, sidebar,
    /// inbox, review and the steps of the assistant all read this result (via `/api/health`).
    /// The stored choice decides which credentials count (see [`AuthChoice::pick`]); without one
    /// a working OAuth sign-in goes before the token and an expired one falls back to it.
    pub fn resolve(
        choice: Option<AuthChoice>,
        token: bool,
        oauth: OAuthTokens,
        oauth_possible: bool,
        site: Option<String>,
    ) -> Self {
        let kind = AuthChoice::pick(choice, token, oauth);
        Self {
            connected: kind != AuthKind::None,
            kind,
            oauth_possible,
            site: site.filter(|n| !n.is_empty() && kind == AuthKind::OAuth),
            expired: oauth == OAuthTokens::Expired,
            choice,
            oauth_stored: oauth != OAuthTokens::Missing,
        }
    }
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
    pub fn agent_blocked(&self) -> Option<cockpit_i18n::Msg> {
        use cockpit_i18n::msg;
        if !self.agent {
            Some(msg!(CoreMiscAgentNotConnected))
        } else if !self.may_branch {
            Some(msg!(CoreMiscAgentNoBranches))
        } else if !self.may_open_pr {
            Some(msg!(CoreMiscAgentNoPullRequests))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
