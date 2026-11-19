//! Where the tokens and the client are stored, and the status of a service.

use super::*;

impl AppState {
    pub(super) fn oauth_config_id(&self, d: Service) -> Option<String> {
        let q = self.sources();
        match d {
            Service::GitHub => q.github_oauth.as_ref().map(|o| o.client_id.clone()),
            Service::Atlassian => q.atlassian.as_ref().map(|a| a.client_id.clone()),
        }
    }

    /// The client secret from the .env file. Blocking.
    pub(super) fn oauth_client_secret(&self, d: Service) -> Option<Secret> {
        match self.inner.secrets.get(secret_name(d)) {
            Ok(s) => s.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Client secret for {} not readable: {e}", d.id());
                None
            }
        }
    }

    /// The tokens from the .env file. Unreadable counts as missing. Blocking.
    pub(crate) fn oauth_tokens(&self, d: Service) -> Option<Tokens> {
        match self.inner.secrets.get(tokens_name(d)) {
            Ok(Some(t)) => {
                let tokens = Tokens::from_text(t.expose());
                if tokens.is_none() {
                    tracing::warn!("Stored tokens for {} are unreadable", d.id());
                }
                tokens
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("Tokens for {} not readable: {e}", d.id());
                None
            }
        }
    }

    pub(super) fn oauth_tokens_set(&self, d: Service, t: &Tokens) -> Result<(), String> {
        self.inner
            .secrets
            .set(tokens_name(d), &Secret::new(t.as_text()))
            .map_err(|e| e.to_string())
    }

    pub(super) fn oauth_tokens_delete(&self, d: Service) {
        if let Err(e) = self.inner.secrets.delete(tokens_name(d)) {
            tracing::warn!("Tokens for {} could not be deleted: {e}", d.id());
        }
        if d == Service::Atlassian
            && let Ok(conn) = self.inner.db.lock()
        {
            for k in [SITE_ID, SITE_NAME, SITE_URL] {
                if let Err(e) = db::setting_delete(&conn, k) {
                    tracing::warn!("Setting {k} could not be deleted: {e}");
                }
            }
        }
    }

    /// The chosen Atlassian site: `(cloudId, name)`. Blocking.
    pub(crate) fn atlassian_site(&self) -> Option<(String, String)> {
        let conn = self.inner.db.lock().ok()?;
        let id = db::setting(&conn, SITE_ID).ok()??;
        let name = db::setting(&conn, SITE_NAME)
            .ok()
            .flatten()
            .unwrap_or_default();
        Some((id, name))
    }

    /// The address of the chosen Atlassian site as people open it, if it was stored (older
    /// sign-ins did not store it). Blocking.
    pub(crate) fn atlassian_site_url(&self) -> Option<String> {
        let conn = self.inner.db.lock().ok()?;
        db::setting(&conn, SITE_URL).ok()?.filter(|u| !u.is_empty())
    }

    /// A Jira client with OAuth if signed in and a site is chosen. Blocking.
    pub(crate) fn jira_oauth_client(&self) -> Option<Result<JiraClient, Error>> {
        let tokens = self.oauth_tokens(Service::Atlassian)?;
        let (id, _) = self.atlassian_site()?;
        // The address for links: the one stored at sign-in, otherwise `[jira] site` of the file.
        let site = self.atlassian_site_url().or_else(|| {
            self.sources()
                .jira
                .as_ref()
                .map(|j| j.site.clone())
                .filter(|s| !s.is_empty())
        });
        Some(
            jira_base(&self.sources().endpoints, &id)
                .and_then(|base| JiraClient::oauth(&base, tokens.access))
                .map(|c| c.with_site(site.as_deref())),
        )
    }

    /// Connection status of a service for `/api/health`. Blocking.
    pub(crate) fn service_status(&self, d: Service, token_present: bool) -> ServiceStatus {
        let oauth_possible =
            self.oauth_config_id(d).is_some() && self.oauth_client_secret(d).is_some();
        let oauth = match self.oauth_tokens(d) {
            // Atlassian also needs the chosen site.
            Some(_) if d == Service::Atlassian && self.atlassian_site().is_none() => {
                OAuthTokens::Missing
            }
            Some(t) if t.refresh.is_none() && t.needs_refresh(now_s()) => OAuthTokens::Expired,
            Some(_) => OAuthTokens::Valid,
            None => OAuthTokens::Missing,
        };
        let site = (d == Service::Atlassian)
            .then(|| self.atlassian_site().map(|(_, n)| n))
            .flatten();
        ServiceStatus::resolve(token_present, oauth, oauth_possible, site)
    }
}
