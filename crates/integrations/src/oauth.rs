//! Sign-in via OAuth: GitHub (GitHub App, user token with refresh) and Atlassian (OAuth 2.0
//! 3LO, Resource-level).
//!
//! The backend redirects the browser to sign in (`state` + PKCE), exchanges the `code` in the callback
//! for tokens and refreshes them before expiry. The client secret and the tokens never leave the
//! backend. Everything here is pure or takes the addresses as [`Endpoints`], so that
//! every case can be tested against a replayed service.
//!
//! This has not been checked against the real services yet.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::http::{client, send};
use crate::{Error, Secret};

mod sign_ins;
pub use sign_ins::*;

/// The services with OAuth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    GitHub,
    Atlassian,
}

impl Service {
    /// This is the service's name in addresses: `/api/oauth/<id>/…`.
    pub fn id(self) -> &'static str {
        match self {
            Service::GitHub => "github",
            Service::Atlassian => "atlassian",
        }
    }

    pub fn from_id(k: &str) -> Option<Self> {
        match k {
            "github" => Some(Service::GitHub),
            "atlassian" => Some(Service::Atlassian),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Service::GitHub => "GitHub",
            Service::Atlassian => "Atlassian",
        }
    }
}

/// Permissions at Atlassian; `offline_access` delivers the refresh token.
pub const ATLASSIAN_SCOPES: &str = "read:jira-work write:jira-work read:jira-user offline_access";

/// The addresses of the services. Default are the real ones; tests insert the replayed service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub github_sign_in: String,
    pub github_token: String,
    pub atlassian_sign_in: String,
    pub atlassian_token: String,
    /// Base of `accessible-resources` and `ex/jira/<cloudId>`.
    pub atlassian_api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            github_sign_in: "https://github.com/login/oauth/authorize".into(),
            github_token: "https://github.com/login/oauth/access_token".into(),
            atlassian_sign_in: "https://auth.atlassian.com/authorize".into(),
            atlassian_token: "https://auth.atlassian.com/oauth/token".into(),
            atlassian_api: "https://api.atlassian.com".into(),
        }
    }
}

/// The credentials of a service's app. The secret is only here and is never printed.
pub struct App<'a> {
    pub client_id: &'a str,
    pub client_secret: &'a Secret,
    pub redirect_uri: &'a str,
}

// ───────────────────────── Tokens ─────────────────────────

/// Access and refresh token of a service. Stored as a single entry in the .env file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: Secret,
    pub refresh: Option<Secret>,
    /// Time (seconds since 1970) from which `access` no longer applies. `None`: does not expire.
    pub expires_in_s: Option<u64>,
}

/// Refresh this early before expiry.
const LEAD_S: u64 = 120;

impl Tokens {
    /// Must it be refreshed before use?
    pub fn needs_refresh(&self, now_s: u64) -> bool {
        self.expires_in_s.is_some_and(|ab| now_s + LEAD_S >= ab)
    }

    /// Text for the .env file.
    pub fn as_text(&self) -> String {
        serde_json::json!({
            "access": self.access.expose(),
            "refresh": self.refresh.as_ref().map(Secret::expose),
            "expires_in_s": self.expires_in_s,
        })
        .to_string()
    }

    /// Counterpart to [`as_text`](Self::as_text); `None` for unreadable or empty token.
    pub fn from_text(text: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Raw {
            access: String,
            #[serde(default)]
            refresh: Option<String>,
            #[serde(default)]
            #[serde(alias = "laeuft_ab_s")]
            expires_in_s: Option<u64>,
        }
        let r: Raw = serde_json::from_str(text).ok()?;
        (!r.access.is_empty()).then(|| Self {
            access: Secret::new(r.access),
            refresh: r.refresh.filter(|t| !t.is_empty()).map(Secret::new),
            expires_in_s: r.expires_in_s,
        })
    }
}

/// Response of the token endpoint. GitHub answers with 200 and `error` even on failures.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Reads the response of the token endpoint. `old`: the previous refresh token, in case the service
/// sends no new one.
fn tokens_from(
    body: &str,
    now_s: u64,
    old: Option<&Secret>,
    secret: &[&str],
) -> Result<Tokens, Error> {
    // Short values would match arbitrary words; real codes and secrets are longer.
    let pure = |m: String| {
        secret
            .iter()
            .filter(|g| g.len() >= 8)
            .fold(m, |m, g| m.replace(g, "***"))
    };
    let a: TokenResponse =
        serde_json::from_str(body).map_err(|e| Error::Response(pure(e.to_string())))?;
    if let Some(error) = a.error {
        let text = a.error_description.unwrap_or(error);
        return Err(Error::Rejected(pure(text.chars().take(240).collect())));
    }
    let access = a
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Error::Response("Kein access_token in der Antwort".into()))?;
    Ok(Tokens {
        access: Secret::new(access),
        refresh: a
            .refresh_token
            .filter(|t| !t.is_empty())
            .map(Secret::new)
            .or_else(|| old.cloned()),
        expires_in_s: a.expires_in.map(|s| now_s + s),
    })
}

// ───────────────────────── Sign-in ──────────────────────

/// `n` random bytes as URL-safe text.
fn random(n: usize) -> Result<String, Error> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|e| Error::Config(format!("Kein Zufall verfügbar: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The PKCE checksum for the `verifier` (method `S256`).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The address the browser is redirected to for sign-in.
pub fn sign_in_url(
    e: &Endpoints,
    service: Service,
    app: &App<'_>,
    state: &str,
    challenge: &str,
) -> Result<String, Error> {
    let mut p: Vec<(&str, &str)> = vec![
        ("client_id", app.client_id),
        ("redirect_uri", app.redirect_uri),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    let base = match service {
        Service::GitHub => &e.github_sign_in,
        Service::Atlassian => {
            p.extend([
                ("audience", "api.atlassian.com"),
                ("scope", ATLASSIAN_SCOPES),
                ("response_type", "code"),
                ("prompt", "consent"),
            ]);
            &e.atlassian_sign_in
        }
    };
    reqwest::Url::parse_with_params(base, &p)
        .map(String::from)
        .map_err(|e| Error::Config(format!("Anmelde-Adresse ungültig: {e}")))
}

/// Exchanges the `code` from the callback for tokens. Runs only in the backend (with the secret).
pub async fn code_exchange(
    e: &Endpoints,
    service: Service,
    app: &App<'_>,
    code: &str,
    verifier: &str,
    now_s: u64,
) -> Result<Tokens, Error> {
    let secret = [app.client_secret.expose(), code, verifier];
    let http = client()?;
    let req = match service {
        Service::GitHub => http
            .post(&e.github_token)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", app.client_id),
                ("client_secret", app.client_secret.expose()),
                ("code", code),
                ("redirect_uri", app.redirect_uri),
                ("code_verifier", verifier),
            ]),
        Service::Atlassian => http
            .post(&e.atlassian_token)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": app.client_id,
                "client_secret": app.client_secret.expose(),
                "code": code,
                "redirect_uri": app.redirect_uri,
                "code_verifier": verifier,
            })),
    };
    let body = send(req).await.map_err(|f| f.without_key(secret[0]))?;
    tokens_from(&body, now_s, None, &secret)
}

/// Fetches new tokens with the refresh token. If it is missing, that is a case of „not signed in“.
pub async fn refresh(
    e: &Endpoints,
    service: Service,
    app: &App<'_>,
    old: &Tokens,
    now_s: u64,
) -> Result<Tokens, Error> {
    let Some(refresh) = old.refresh.as_ref() else {
        return Err(Error::NotSignedIn);
    };
    let secret = [app.client_secret.expose(), refresh.expose()];
    let http = client()?;
    let req = match service {
        Service::GitHub => http
            .post(&e.github_token)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", app.client_id),
                ("client_secret", app.client_secret.expose()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.expose()),
            ]),
        Service::Atlassian => http
            .post(&e.atlassian_token)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": app.client_id,
                "client_secret": app.client_secret.expose(),
                "refresh_token": refresh.expose(),
            })),
    };
    let body = send(req).await.map_err(|f| f.without_key(secret[0]))?;
    // Atlassian rotates the refresh token on every refresh: the new one replaces the old.
    tokens_from(&body, now_s, Some(refresh), &secret)
}

/// Revokes the token at the service. Only GitHub offers this as a call (`DELETE
/// /applications/<client_id>/token`); at Atlassian the revocation stays in the account settings
/// (`Ok(false)`: nothing revoked at the service).
pub async fn revoked(
    service: Service,
    app: &App<'_>,
    github_api: &str,
    tokens: &Tokens,
) -> Result<bool, Error> {
    match service {
        Service::Atlassian => Ok(false),
        Service::GitHub => {
            let base = crate::http::base_url(github_api)?;
            let req = client()?
                .delete(format!("{base}/applications/{}/token", app.client_id))
                .basic_auth(app.client_id, Some(app.client_secret.expose()))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&serde_json::json!({ "access_token": tokens.access.expose() }));
            match send(req).await {
                Ok(_) => Ok(true),
                // Already revoked or expired: goal reached.
                Err(Error::NotFound) => Ok(true),
                Err(f) => Err(f
                    .without_key(app.client_secret.expose())
                    .without_key(tokens.access.expose())),
            }
        }
    }
}

// ───────────────────────── Atlassian: Site ─────────────────────────

/// A Jira site the consent applies to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// `GET /oauth/token/accessible-resources`: the sites the user has allowed.
pub async fn sites(e: &Endpoints, access: &Secret) -> Result<Vec<Site>, Error> {
    let base = crate::http::base_url(&e.atlassian_api)?;
    let req = client()?
        .get(format!("{base}/oauth/token/accessible-resources"))
        .bearer_auth(access.expose())
        .header("Accept", "application/json");
    let body = send(req)
        .await
        .map_err(|f| f.without_key(access.expose()))?;
    serde_json::from_str(&body).map_err(|e| Error::Response(e.to_string()))
}

/// Chooses the site: exactly one automatically, otherwise by `requested` (name, address or ID from
/// the configuration). With several and no request the error names the options.
pub fn site_choose(sites: &[Site], requested: Option<&str>) -> Result<Site, Error> {
    let names = || {
        sites
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (sites, requested.map(str::trim).filter(|g| !g.is_empty())) {
        ([], _) => Err(Error::NotPossible(
            "Atlassian hat keine Jira-Site freigegeben – bei der Zustimmung eine Site wählen"
                .into(),
        )),
        (list, Some(requested)) => {
            let w = requested.trim_end_matches('/').to_ascii_lowercase();
            list.iter()
                .find(|s| {
                    s.id.eq_ignore_ascii_case(&w)
                        || s.name.to_ascii_lowercase() == w
                        || s.url.trim_end_matches('/').to_ascii_lowercase() == w
                })
                .cloned()
                .ok_or_else(|| {
                    Error::NotPossible(format!(
                        "Die Site „{requested}“ ist nicht freigegeben. Freigegeben: {}",
                        names()
                    ))
                })
        }
        ([eine], None) => Ok(eine.clone()),
        (_, None) => Err(Error::NotPossible(format!(
            "Mehrere Sites freigegeben ({}) – in config.toml unter [atlassian] `site` setzen",
            names()
        ))),
    }
}

/// The base for Jira via the Atlassian gateway: `<api>/ex/jira/<cloudId>`.
pub fn jira_base(e: &Endpoints, cloud_id: &str) -> Result<String, Error> {
    let ok = !cloud_id.is_empty()
        && cloud_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !ok {
        return Err(Error::Config("Ungültige Site-Id".into()));
    }
    Ok(format!(
        "{}/ex/jira/{cloud_id}",
        crate::http::base_url(&e.atlassian_api)?
    ))
}

#[cfg(test)]
mod tests;
