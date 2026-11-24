//! Refreshing tokens that expire soon.

use cockpit_integrations::Cause;

use super::*;

impl AppState {
    /// Refreshes tokens that expire soon. If that fails because the service no longer accepts the refresh token,
    /// the tokens are deleted: the service counts as disconnected.
    /// Network errors and outages of the service leave everything as it is.
    pub async fn oauth_refresh(&self) {
        for d in [Service::GitHub, Service::Atlassian] {
            if self.oauth_config_id(d).is_some() {
                self.oauth_refresh_for(d).await;
            }
        }
    }

    async fn oauth_refresh_for(&self, d: Service) {
        // Atlassian rotates the refresh token: two refreshes at the same time would invalidate
        // each other.
        let _lock = self.inner.refresh.lock().await;
        let s = self.clone();
        let Ok((Some(tokens), secret)) =
            tokio::task::spawn_blocking(move || (s.oauth_tokens(d), s.oauth_client_secret(d)))
                .await
        else {
            return;
        };
        if !tokens.needs_refresh(now_s()) {
            return;
        }
        let (Some(secret), Some(client_id)) = (secret, self.oauth_config_id(d)) else {
            tracing::warn!("{}: client secret missing, tokens not refreshed", d.id());
            return;
        };
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        let result = oauth::refresh(&self.sources().endpoints, d, &app, &tokens, now_s()).await;
        let s = self.clone();
        match result {
            Ok(updated) => {
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = s.oauth_tokens_set(d, &updated) {
                        tracing::warn!("{}: new tokens could not be stored: {e}", d.id());
                    }
                })
                .await;
            }
            Err(f) if tokens_unusable(&f) => {
                tracing::warn!("{}: refresh rejected ({f}) - disconnected", d.id());
                let _ = tokio::task::spawn_blocking(move || s.oauth_tokens_delete(d)).await;
            }
            Err(f) => tracing::warn!("{}: refresh failed: {f}", d.id()),
        }
    }
}

/// Does the failure of a refresh mean that the service no longer accepts the stored tokens?
/// Only then they are deleted; an outage or a network error leaves them as they are.
pub(super) fn tokens_unusable(f: &Error) -> bool {
    matches!(
        f.cause(),
        Cause::Token | Cause::Rejected | Cause::Permission | Cause::BadResponse
    )
}
