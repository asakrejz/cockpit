//! The sign-in flow: start, callback and revoke.

use super::*;

impl AppState {
    /// Begins a sign-in and returns the address of the service.
    pub async fn oauth_start(&self, d: Service) -> Result<String, String> {
        let Some(client_id) = self.oauth_config_id(d) else {
            return Err(format!(
                "{}-Anmeldung ist nicht eingerichtet – Client-ID in config.toml eintragen",
                d.name()
            ));
        };
        let s = self.clone();
        let secret = tokio::task::spawn_blocking(move || s.oauth_client_secret(d))
            .await
            .map_err(|_| "Interner Fehler".to_string())?;
        let Some(secret) = secret else {
            return Err(format!(
                "Das Client-Secret fehlt – `{}` ausführen",
                command(d)
            ));
        };
        let (state, challenge) = self
            .inner
            .sign_ins
            .lock()
            .map_err(|_| "Interner Fehler".to_string())?
            .begin(d, now_ms())
            .map_err(|e| e.to_string())?;
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        oauth::sign_in_url(&self.sources().endpoints, d, &app, &state, &challenge)
            .map_err(|e| e.to_string())
    }

    /// Processes the callback. `Ok`: a success message, `Err`: an understandable message.
    pub async fn oauth_callback(
        &self,
        d: Service,
        q: &HashMap<String, String>,
    ) -> Result<String, String> {
        let get = |k: &str| q.get(k).map(String::as_str).filter(|v| !v.is_empty());
        let state = get("state");
        // The user declined or the service reports an error: consume `state`.
        if let Some(error) = get("error") {
            if let (Some(st), Ok(mut a)) = (state, self.inner.sign_ins.lock()) {
                let _ = a.extract(st, d, now_ms());
            }
            let reason: String = get("error_description")
                .unwrap_or(error)
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            return Err(format!(
                "{}-Anmeldung abgebrochen oder abgelehnt: {reason}",
                d.name()
            ));
        }
        let Some(state) = state else {
            return Err(format!(
                "Keine laufende Anmeldung – bitte in Cockpit unter „Einrichten“ auf „Mit {} anmelden“ klicken",
                d.name()
            ));
        };
        let verifier = self
            .inner
            .sign_ins
            .lock()
            .map_err(|_| "Interner Fehler".to_string())?
            .extract(state, d, now_ms())
            .map_err(|e| e.text().to_string())?;
        let Some(code) = get("code") else {
            return Err(format!("{} hat keinen Code geschickt", d.name()));
        };
        let Some(client_id) = self.oauth_config_id(d) else {
            return Err(format!("{}-Anmeldung ist nicht eingerichtet", d.name()));
        };
        let s = self.clone();
        let secret = tokio::task::spawn_blocking(move || s.oauth_client_secret(d))
            .await
            .map_err(|_| "Interner Fehler".to_string())?
            .ok_or_else(|| format!("Das Client-Secret fehlt – `{}` ausführen", command(d)))?;
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        let sources = self.sources();
        let e = &sources.endpoints;
        let tokens = oauth::code_exchange(e, d, &app, code, &verifier, now_s())
            .await
            .map_err(|f| {
                tracing::warn!("{}: Austausch fehlgeschlagen: {f}", d.id());
                service_error(d, &f)
            })?;

        // Atlassian: first the site, then it is saved – no half state.
        let site: Option<Site> = if d == Service::Atlassian {
            let wanted = sources.atlassian.as_ref().and_then(|a| a.site.clone());
            let list = oauth::sites(e, &tokens.access).await.map_err(|f| {
                tracing::warn!("Atlassian: accessible-resources fehlgeschlagen: {f}");
                service_error(d, &f)
            })?;
            Some(site_choose(&list, wanted.as_deref()).map_err(|f| f.to_string())?)
        } else {
            None
        };

        let s = self.clone();
        let name = site.as_ref().map(|x| x.name.clone());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            s.oauth_tokens_set(d, &tokens).map_err(|e| {
                format!("Die Tokens ließen sich nicht in der .env-Datei ablegen: {e}")
            })?;
            if let Some(site) = site {
                let conn = s
                    .inner
                    .db
                    .lock()
                    .map_err(|_| "Interner Fehler".to_string())?;
                let r = db::setting_set(&conn, SITE_ID, &site.id)
                    .and_then(|()| db::setting_set(&conn, SITE_NAME, &site.name));
                if let Err(e) = r {
                    drop(conn);
                    s.oauth_tokens_delete(d);
                    return Err(format!("Die Site ließ sich nicht speichern: {e}"));
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| "Interner Fehler".to_string())??;
        tracing::info!("{}: angemeldet", d.id());

        // Sync right away; the result comes through the event stream.
        let s = self.clone();
        tokio::spawn(async move {
            if let Err(e) = s.sync().await {
                tracing::warn!("Sync after sign-in: {e:?}");
            }
        });
        Ok(match name {
            Some(n) => format!("Mit Atlassian angemeldet (Site {n})"),
            None => format!("Mit {} angemeldet", d.name()),
        })
    }

    /// Disconnects the service: calls, where it exists, the revocation at the service and deletes the tokens
    /// in the .env file. Deleting always happens – even if the service does not answer.
    pub async fn oauth_revoke(&self, d: Service) -> Result<RevokeResponse, String> {
        let s = self.clone();
        let (tokens, secret) =
            tokio::task::spawn_blocking(move || (s.oauth_tokens(d), s.oauth_client_secret(d)))
                .await
                .map_err(|_| "Interner Fehler".to_string())?;
        let mut answer = RevokeResponse::default();
        if let (Some(tokens), Some(secret), Some(client_id)) =
            (&tokens, &secret, self.oauth_config_id(d))
        {
            let uri = redirect_uri(d, self.inner.port);
            let app = App {
                client_id: &client_id,
                client_secret: secret,
                redirect_uri: &uri,
            };
            let sources = self.sources();
            let api = sources
                .github
                .as_ref()
                .map_or(crate::config::DEFAULT_GITHUB_API, |g| g.api_url.as_str());
            match oauth::revoked(d, &app, api, tokens).await {
                Ok(at_service) => answer.at_service = at_service,
                Err(f) => {
                    tracing::warn!("{}: Widerruf beim Dienst fehlgeschlagen: {f}", d.id());
                    answer.hint = Some(format!(
                        "Lokal getrennt, aber der Dienst hat den Widerruf nicht bestätigt ({f}). Zugriff bitte in den Einstellungen bei {} entziehen.",
                        d.name()
                    ));
                }
            }
        }
        if d == Service::Atlassian && answer.hint.is_none() {
            answer.hint = Some(
                "Lokal getrennt. Atlassian bietet keinen Widerruf per Aufruf – den Zugriff entziehst du unter id.atlassian.com/manage-profile/apps."
                    .into(),
            );
        }
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.oauth_tokens_delete(d))
            .await
            .map_err(|_| "Interner Fehler".to_string())?;
        answer.disconnected = true;
        tracing::info!("{}: getrennt", d.id());
        Ok(answer)
    }
}
