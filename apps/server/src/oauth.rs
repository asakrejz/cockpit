//! Anmeldung per OAuth im Backend: Weiterleitung, Callback, Auffrischen, Widerruf.
//!
//! Die Regeln (PKCE, `state`, Austausch, Site-Wahl) stehen in `cockpit_integrations::oauth`;
//! hier läuft der Ablauf mit .env-Datei und Datenbank. Client-Secret und Tokens liegen nur im
//! .env-Datei, nie in der Datenbank, nie im Log, nie im Browser. Der Browser sieht nur
//! Weiterleitungen und Meldungen.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use cockpit_core::api::{ErrorBody, RevokeResponse};
use cockpit_core::{AuthKind, ServiceStatus};
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::{
    self, App, Service, SignIns, Site, Tokens, jira_base, site_choose,
};
use cockpit_integrations::{Error, Secret};

use crate::app::{AppState, jetzt_ms};
use crate::db;
use crate::secrets;

const SITE_ID: &str = "atlassian.cloud_id";
const SITE_NAME: &str = "atlassian.site_name";

fn secret_name(d: Service) -> &'static str {
    match d {
        Service::GitHub => secrets::GITHUB_OAUTH,
        Service::Atlassian => secrets::ATLASSIAN_OAUTH,
    }
}

fn tokens_name(d: Service) -> &'static str {
    match d {
        Service::GitHub => secrets::GITHUB_OAUTH_TOKENS,
        Service::Atlassian => secrets::ATLASSIAN_OAUTH_TOKENS,
    }
}

fn befehl(d: Service) -> String {
    format!("cockpit-server token setzen {}", secret_name(d))
}

/// Die Callback-Adresse, wie sie in der App des Dienstes eingetragen ist. GitHub: `127.0.0.1`,
/// Atlassian: `localhost`.
pub fn redirect_uri(d: Service, port: u16) -> String {
    let host = match d {
        Service::GitHub => "127.0.0.1",
        Service::Atlassian => "localhost",
    };
    format!("http://{host}:{port}/api/oauth/{}/callback", d.id())
}

fn jetzt_s() -> u64 {
    jetzt_ms() / 1000
}

/// Eine Meldung für die Oberfläche; enthält nie einen Schlüssel.
fn dienst_fehler(d: Service, f: &Error) -> String {
    match f {
        Error::NotSignedIn => format!(
            "{} lehnt die Zugangsdaten ab – Client-ID in config.toml und Client-Secret in der .env-Datei prüfen",
            d.name()
        ),
        Error::Network(_) => format!("{} ist nicht erreichbar", d.name()),
        andere => format!("{}: {andere}", d.name()),
    }
}

/// Prozentkodierung für Teile einer Adresse.
fn kodiert(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl AppState {
    fn oauth_konfig_id(&self, d: Service) -> Option<String> {
        let q = self.quellen();
        match d {
            Service::GitHub => q.github_oauth.as_ref().map(|o| o.client_id.clone()),
            Service::Atlassian => q.atlassian.as_ref().map(|a| a.client_id.clone()),
        }
    }

    /// Das Client-Secret aus der .env-Datei. Blockierend.
    fn oauth_client_secret(&self, d: Service) -> Option<Secret> {
        match self.inner.secrets.get(secret_name(d)) {
            Ok(s) => s.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Client-Secret für {} nicht lesbar: {e}", d.id());
                None
            }
        }
    }

    /// Die Tokens aus der .env-Datei. Unlesbares zählt als fehlend. Blockierend.
    pub(crate) fn oauth_tokens(&self, d: Service) -> Option<Tokens> {
        match self.inner.secrets.get(tokens_name(d)) {
            Ok(Some(t)) => {
                let tokens = Tokens::from_text(t.expose());
                if tokens.is_none() {
                    tracing::warn!("Gespeicherte Tokens für {} sind unlesbar", d.id());
                }
                tokens
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("Tokens für {} nicht lesbar: {e}", d.id());
                None
            }
        }
    }

    fn oauth_tokens_setzen(&self, d: Service, t: &Tokens) -> Result<(), String> {
        self.inner
            .secrets
            .set(tokens_name(d), &Secret::new(t.as_text()))
            .map_err(|e| e.to_string())
    }

    fn oauth_tokens_loeschen(&self, d: Service) {
        if let Err(e) = self.inner.secrets.delete(tokens_name(d)) {
            tracing::warn!("Tokens für {} nicht löschbar: {e}", d.id());
        }
        if d == Service::Atlassian
            && let Ok(conn) = self.inner.db.lock()
        {
            for k in [SITE_ID, SITE_NAME] {
                if let Err(e) = db::einstellung_loeschen(&conn, k) {
                    tracing::warn!("Einstellung {k} nicht löschbar: {e}");
                }
            }
        }
    }

    /// Die gewählte Atlassian-Site: `(cloudId, Name)`. Blockierend.
    pub(crate) fn atlassian_site(&self) -> Option<(String, String)> {
        let conn = self.inner.db.lock().ok()?;
        let id = db::einstellung(&conn, SITE_ID).ok()??;
        let name = db::einstellung(&conn, SITE_NAME)
            .ok()
            .flatten()
            .unwrap_or_default();
        Some((id, name))
    }

    /// Ein Jira-Client mit OAuth, falls angemeldet und eine Site gewählt ist. Blockierend.
    pub(crate) fn jira_oauth_client(&self) -> Option<Result<JiraClient, Error>> {
        let tokens = self.oauth_tokens(Service::Atlassian)?;
        let (id, _) = self.atlassian_site()?;
        Some(
            jira_base(&self.quellen().endpunkte, &id)
                .and_then(|base| JiraClient::oauth(&base, tokens.access)),
        )
    }

    /// Verbindungsstatus eines Dienstes für `/api/health`. Blockierend.
    pub(crate) fn dienst_status(&self, d: Service, token_vorhanden: bool) -> ServiceStatus {
        let oauth_moeglich =
            self.oauth_konfig_id(d).is_some() && self.oauth_client_secret(d).is_some();
        let oauth_verbunden = self.oauth_tokens(d).is_some()
            && (d == Service::GitHub || self.atlassian_site().is_some());
        let (verbunden, art) = if oauth_verbunden {
            (true, AuthKind::OAuth)
        } else if token_vorhanden {
            (true, AuthKind::Token)
        } else {
            (false, AuthKind::None)
        };
        ServiceStatus {
            connected: verbunden,
            kind: art,
            oauth_possible: oauth_moeglich,
            site: (d == Service::Atlassian && oauth_verbunden)
                .then(|| self.atlassian_site().map(|(_, n)| n))
                .flatten()
                .filter(|n| !n.is_empty()),
        }
    }

    // ───────────── Ablauf ─────────────

    /// Beginnt eine Anmeldung und liefert die Adresse des Dienstes.
    pub async fn oauth_start(&self, d: Service) -> Result<String, String> {
        let Some(client_id) = self.oauth_konfig_id(d) else {
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
                befehl(d)
            ));
        };
        let (state, challenge) = self
            .inner
            .anmeldungen
            .lock()
            .map_err(|_| "Interner Fehler".to_string())?
            .begin(d, jetzt_ms())
            .map_err(|e| e.to_string())?;
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        oauth::sign_in_url(&self.quellen().endpunkte, d, &app, &state, &challenge)
            .map_err(|e| e.to_string())
    }

    /// Verarbeitet den Callback. `Ok`: eine Erfolgsmeldung, `Err`: eine verständliche Meldung.
    pub async fn oauth_callback(
        &self,
        d: Service,
        q: &HashMap<String, String>,
    ) -> Result<String, String> {
        let get = |k: &str| q.get(k).map(String::as_str).filter(|v| !v.is_empty());
        let state = get("state");
        // Der Nutzer hat abgelehnt oder der Dienst meldet einen Fehler: `state` verbrauchen.
        if let Some(fehler) = get("error") {
            if let (Some(st), Ok(mut a)) = (state, self.inner.anmeldungen.lock()) {
                let _ = a.extract(st, d, jetzt_ms());
            }
            let grund: String = get("error_description")
                .unwrap_or(fehler)
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            return Err(format!(
                "{}-Anmeldung abgebrochen oder abgelehnt: {grund}",
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
            .anmeldungen
            .lock()
            .map_err(|_| "Interner Fehler".to_string())?
            .extract(state, d, jetzt_ms())
            .map_err(|e| e.text().to_string())?;
        let Some(code) = get("code") else {
            return Err(format!("{} hat keinen Code geschickt", d.name()));
        };
        let Some(client_id) = self.oauth_konfig_id(d) else {
            return Err(format!("{}-Anmeldung ist nicht eingerichtet", d.name()));
        };
        let s = self.clone();
        let secret = tokio::task::spawn_blocking(move || s.oauth_client_secret(d))
            .await
            .map_err(|_| "Interner Fehler".to_string())?
            .ok_or_else(|| format!("Das Client-Secret fehlt – `{}` ausführen", befehl(d)))?;
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        let quellen = self.quellen();
        let e = &quellen.endpunkte;
        let tokens = oauth::code_exchange(e, d, &app, code, &verifier, jetzt_s())
            .await
            .map_err(|f| {
                tracing::warn!("{}: Austausch fehlgeschlagen: {f}", d.id());
                dienst_fehler(d, &f)
            })?;

        // Atlassian: erst die Site, dann wird gespeichert – kein halber Zustand.
        let site: Option<Site> = if d == Service::Atlassian {
            let gewuenscht = quellen.atlassian.as_ref().and_then(|a| a.site.clone());
            let liste = oauth::sites(e, &tokens.access).await.map_err(|f| {
                tracing::warn!("Atlassian: accessible-resources fehlgeschlagen: {f}");
                dienst_fehler(d, &f)
            })?;
            Some(site_choose(&liste, gewuenscht.as_deref()).map_err(|f| f.to_string())?)
        } else {
            None
        };

        let s = self.clone();
        let name = site.as_ref().map(|x| x.name.clone());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            s.oauth_tokens_setzen(d, &tokens).map_err(|e| {
                format!("Die Tokens ließen sich nicht in der .env-Datei ablegen: {e}")
            })?;
            if let Some(site) = site {
                let conn = s
                    .inner
                    .db
                    .lock()
                    .map_err(|_| "Interner Fehler".to_string())?;
                let r = db::einstellung_setzen(&conn, SITE_ID, &site.id)
                    .and_then(|()| db::einstellung_setzen(&conn, SITE_NAME, &site.name));
                if let Err(e) = r {
                    drop(conn);
                    s.oauth_tokens_loeschen(d);
                    return Err(format!("Die Site ließ sich nicht speichern: {e}"));
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| "Interner Fehler".to_string())??;
        tracing::info!("{}: angemeldet", d.id());

        // Gleich abgleichen; das Ergebnis kommt über den Ereignisstrom.
        let s = self.clone();
        tokio::spawn(async move {
            if let Err(e) = s.abgleichen().await {
                tracing::warn!("Abgleich nach der Anmeldung: {e:?}");
            }
        });
        Ok(match name {
            Some(n) => format!("Mit Atlassian angemeldet (Site {n})"),
            None => format!("Mit {} angemeldet", d.name()),
        })
    }

    /// Trennt den Dienst: ruft, wo es das gibt, den Widerruf beim Dienst auf und löscht die Tokens
    /// in der .env-Datei. Gelöscht wird immer – auch wenn der Dienst nicht antwortet.
    pub async fn oauth_widerrufen(&self, d: Service) -> Result<RevokeResponse, String> {
        let s = self.clone();
        let (tokens, secret) =
            tokio::task::spawn_blocking(move || (s.oauth_tokens(d), s.oauth_client_secret(d)))
                .await
                .map_err(|_| "Interner Fehler".to_string())?;
        let mut antwort = RevokeResponse::default();
        if let (Some(tokens), Some(secret), Some(client_id)) =
            (&tokens, &secret, self.oauth_konfig_id(d))
        {
            let uri = redirect_uri(d, self.inner.port);
            let app = App {
                client_id: &client_id,
                client_secret: secret,
                redirect_uri: &uri,
            };
            let quellen = self.quellen();
            let api = quellen
                .github
                .as_ref()
                .map_or(crate::config::STANDARD_GITHUB_API, |g| g.api_url.as_str());
            match oauth::revoked(d, &app, api, tokens).await {
                Ok(beim_dienst) => antwort.at_service = beim_dienst,
                Err(f) => {
                    tracing::warn!("{}: Widerruf beim Dienst fehlgeschlagen: {f}", d.id());
                    antwort.hint = Some(format!(
                        "Lokal getrennt, aber der Dienst hat den Widerruf nicht bestätigt ({f}). Zugriff bitte in den Einstellungen bei {} entziehen.",
                        d.name()
                    ));
                }
            }
        }
        if d == Service::Atlassian && antwort.hint.is_none() {
            antwort.hint = Some(
                "Lokal getrennt. Atlassian bietet keinen Widerruf per Aufruf – den Zugriff entziehst du unter id.atlassian.com/manage-profile/apps."
                    .into(),
            );
        }
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.oauth_tokens_loeschen(d))
            .await
            .map_err(|_| "Interner Fehler".to_string())?;
        antwort.disconnected = true;
        tracing::info!("{}: getrennt", d.id());
        Ok(antwort)
    }

    /// Frischt Tokens auf, die bald ablaufen. Scheitert das, weil der Dienst das Refresh-Token
    /// nicht mehr gelten lässt, werden die Tokens gelöscht: Der Dienst gilt als getrennt.
    /// Netzwerkfehler und Störungen des Dienstes lassen alles, wie es ist.
    pub async fn oauth_auffrischen(&self) {
        for d in [Service::GitHub, Service::Atlassian] {
            if self.oauth_konfig_id(d).is_some() {
                self.oauth_auffrischen_fuer(d).await;
            }
        }
    }

    async fn oauth_auffrischen_fuer(&self, d: Service) {
        // Atlassian dreht das Refresh-Token: Zwei Auffrischungen gleichzeitig würden sich
        // gegenseitig ungültig machen.
        let _sperre = self.inner.auffrischen.lock().await;
        let s = self.clone();
        let Ok((Some(tokens), secret)) =
            tokio::task::spawn_blocking(move || (s.oauth_tokens(d), s.oauth_client_secret(d)))
                .await
        else {
            return;
        };
        if !tokens.needs_refresh(jetzt_s()) {
            return;
        }
        let (Some(secret), Some(client_id)) = (secret, self.oauth_konfig_id(d)) else {
            tracing::warn!("{}: Client-Secret fehlt, Tokens nicht aufgefrischt", d.id());
            return;
        };
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        let ergebnis = oauth::refresh(&self.quellen().endpunkte, d, &app, &tokens, jetzt_s()).await;
        let s = self.clone();
        match ergebnis {
            Ok(neu) => {
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = s.oauth_tokens_setzen(d, &neu) {
                        tracing::warn!("{}: neue Tokens nicht ablegbar: {e}", d.id());
                    }
                })
                .await;
            }
            Err(
                f @ (Error::NotSignedIn
                | Error::Rejected(_)
                | Error::NoPermission
                | Error::Response(_)),
            ) => {
                tracing::warn!("{}: Auffrischen abgelehnt ({f}) – getrennt", d.id());
                let _ = tokio::task::spawn_blocking(move || s.oauth_tokens_loeschen(d)).await;
            }
            Err(f) => tracing::warn!("{}: Auffrischen nicht möglich: {f}", d.id()),
        }
    }
}

// ───────────── Routen ─────────────

fn dienst_aus(kennung: &str) -> Result<Service, ()> {
    Service::from_id(kennung).ok_or(())
}

fn unbekannter_dienst() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: "Unbekannter Dienst".into(),
        }),
    )
        .into_response()
}

/// Zurück in die App – über die Adresse `127.0.0.1`, damit der gespeicherte Modus des Browsers
/// gilt, auch wenn der Dienst auf `localhost` zurückgeleitet hat.
fn zurueck(port: u16, d: Service, ergebnis: &Result<String, String>) -> Redirect {
    let basis = format!("http://127.0.0.1:{port}/");
    let ziel = match ergebnis {
        Ok(m) => format!(
            "{basis}?oauth=ok&dienst={}&meldung={}#/einrichten",
            d.id(),
            kodiert(m)
        ),
        Err(m) => format!(
            "{basis}?oauth=fehler&dienst={}&meldung={}#/einrichten",
            d.id(),
            kodiert(m)
        ),
    };
    Redirect::to(&ziel)
}

/// `GET /api/oauth/{dienst}/start`: leitet zur Anmeldung des Dienstes weiter.
pub async fn start(State(state): State<AppState>, Path(dienst): Path<String>) -> Response {
    let Ok(d) = dienst_aus(&dienst) else {
        return unbekannter_dienst();
    };
    match state.oauth_start(d).await {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(m) => zurueck(state.inner.port, d, &Err(m)).into_response(),
    }
}

/// `GET /api/oauth/{dienst}/callback`: nimmt den Browser nach der Anmeldung entgegen.
pub async fn callback(
    State(state): State<AppState>,
    Path(dienst): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Ok(d) = dienst_aus(&dienst) else {
        return unbekannter_dienst();
    };
    let ergebnis = state.oauth_callback(d, &q).await;
    if let Err(m) = &ergebnis {
        tracing::warn!("{}: Anmeldung gescheitert: {m}", d.id());
    }
    zurueck(state.inner.port, d, &ergebnis).into_response()
}

/// `POST /api/oauth/{dienst}/widerrufen`: trennt den Dienst.
pub async fn widerrufen(State(state): State<AppState>, Path(dienst): Path<String>) -> Response {
    let Ok(d) = dienst_aus(&dienst) else {
        return unbekannter_dienst();
    };
    match state.oauth_widerrufen(d).await {
        Ok(a) => Json(a).into_response(),
        Err(m) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody { error: m }),
        )
            .into_response(),
    }
}

/// Zustand für `AppState`: offene Anmeldungen.
pub fn neue_anmeldungen() -> std::sync::Mutex<SignIns> {
    std::sync::Mutex::new(SignIns::default())
}

#[cfg(test)]
mod tests;
