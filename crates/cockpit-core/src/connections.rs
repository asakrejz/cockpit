//! Connections to Jira and GitHub, as the backend reports them.
//!
//! [`ServiceStatus`] holds only checkmarks – **never keys**.

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

use crate::setup::Tone;

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

/// The sign-in method the user picked for a service (`auth = "token" | "oauth"` in the
/// `[github]` / `[jira]` section). Without a stored choice a working OAuth sign-in goes first,
/// then the token.
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

    /// **The one rule** for which credentials a service uses.
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
    /// "Sign in with …" is possible.
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
    /// **The one place** that decides whether a service counts as connected. The stored choice
    /// decides which credentials count (see [`AuthChoice::pick`]).
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

    /// **The one place** that turns the state of a connection into a tone and a sentence.
    /// `provider` names the service the sign-in goes through ("GitHub", "Atlassian"). A service
    /// that is not connected and whose sign-in did not run out is [`Tone::Open`]; what is missing
    /// to connect it depends on the setup.
    pub fn summary(&self, provider: &str) -> (Tone, Msg) {
        match (self.connected, self.kind) {
            // The sign-in ran out and no token takes over: say why, not "not connected".
            (false, _) if self.expired => (Tone::Problem, msg!(CoreSetupExpired)),
            (true, AuthKind::OAuth) => match &self.site {
                Some(site) => (
                    Tone::Good,
                    msg!(CoreSetupConnectedOauthSite, name = provider, site = site),
                ),
                None => (Tone::Good, msg!(CoreSetupConnectedOauth, name = provider)),
            },
            (true, _) => (Tone::Good, msg!(CoreSetupConnectedToken)),
            (false, _) => (Tone::Open, msg!(CoreSetupDisconnected)),
        }
    }

    /// Nothing more to say about the setup: connected, or the sign-in ran out.
    pub(crate) fn is_settled(&self) -> bool {
        self.connected || self.expired
    }
}

#[cfg(test)]
mod tests;
