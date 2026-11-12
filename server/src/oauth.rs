//! Anmeldung per OAuth im Backend: Weiterleitung, Callback, Auffrischen, Widerruf.
//!
//! Die Regeln (PKCE, `state`, Austausch, Site-Wahl) stehen in `cockpit_integrations::oauth`;
//! hier läuft der Ablauf mit Schlüsselbund und Datenbank. Client-Secret und Tokens liegen nur im
//! Schlüsselbund, nie in der Datenbank, nie im Log, nie im Browser. Der Browser sieht nur
//! Weiterleitungen und Meldungen.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use cockpit_core::api::{ErrorBody, WiderrufAntwort};
use cockpit_core::{AuthKind, ServiceStatus};
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::{
    self, Anmeldungen, App, Dienst, Site, Tokens, jira_basis, site_waehlen,
};
use cockpit_integrations::{Fehler, Secret};

use crate::app::{AppState, jetzt_ms};
use crate::db;
use crate::secrets;

const SITE_ID: &str = "atlassian.cloud_id";
const SITE_NAME: &str = "atlassian.site_name";

fn secret_name(d: Dienst) -> &'static str {
    match d {
        Dienst::GitHub => secrets::GITHUB_OAUTH,
        Dienst::Atlassian => secrets::ATLASSIAN_OAUTH,
    }
}

fn tokens_name(d: Dienst) -> &'static str {
    match d {
        Dienst::GitHub => secrets::GITHUB_OAUTH_TOKENS,
        Dienst::Atlassian => secrets::ATLASSIAN_OAUTH_TOKENS,
    }
}

fn befehl(d: Dienst) -> String {
    format!("cockpit-server token setzen {}", secret_name(d))
}

/// Die Callback-Adresse, wie sie in der App des Dienstes eingetragen ist. GitHub: `127.0.0.1`,
/// Atlassian: `localhost`.
pub fn redirect_uri(d: Dienst, port: u16) -> String {
    let host = match d {
        Dienst::GitHub => "127.0.0.1",
        Dienst::Atlassian => "localhost",
    };
    format!("http://{host}:{port}/api/oauth/{}/callback", d.kennung())
}

fn jetzt_s() -> u64 {
    jetzt_ms() / 1000
}

/// Eine Meldung für die Oberfläche; enthält nie einen Schlüssel.
fn dienst_fehler(d: Dienst, f: &Fehler) -> String {
    match f {
        Fehler::NichtAngemeldet => format!(
            "{} lehnt die Zugangsdaten ab – Client-ID in config.toml und Client-Secret im Schlüsselbund prüfen",
            d.name()
        ),
        Fehler::Netzwerk(_) => format!("{} ist nicht erreichbar", d.name()),
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
    fn oauth_konfig_id(&self, d: Dienst) -> Option<String> {
        let q = &self.inner.quellen;
        match d {
            Dienst::GitHub => q.github_oauth.as_ref().map(|o| o.client_id.clone()),
            Dienst::Atlassian => q.atlassian.as_ref().map(|a| a.client_id.clone()),
        }
    }

    /// Das Client-Secret aus dem Schlüsselbund. Blockierend.
    fn oauth_client_secret(&self, d: Dienst) -> Option<Secret> {
        match self.inner.secrets.get(secret_name(d)) {
            Ok(s) => s.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Client-Secret für {} nicht lesbar: {e}", d.kennung());
                None
            }
        }
    }

    /// Die Tokens aus dem Schlüsselbund. Unlesbares zählt als fehlend. Blockierend.
    pub(crate) fn oauth_tokens(&self, d: Dienst) -> Option<Tokens> {
        match self.inner.secrets.get(tokens_name(d)) {
            Ok(Some(t)) => {
                let tokens = Tokens::aus_text(t.expose());
                if tokens.is_none() {
                    tracing::warn!("Gespeicherte Tokens für {} sind unlesbar", d.kennung());
                }
                tokens
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("Tokens für {} nicht lesbar: {e}", d.kennung());
                None
            }
        }
    }

    fn oauth_tokens_setzen(&self, d: Dienst, t: &Tokens) -> Result<(), String> {
        self.inner
            .secrets
            .set(tokens_name(d), &Secret::new(t.als_text()))
            .map_err(|e| e.to_string())
    }

    fn oauth_tokens_loeschen(&self, d: Dienst) {
        if let Err(e) = self.inner.secrets.delete(tokens_name(d)) {
            tracing::warn!("Tokens für {} nicht löschbar: {e}", d.kennung());
        }
        if d == Dienst::Atlassian
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
    pub(crate) fn jira_oauth_client(&self) -> Option<Result<JiraClient, Fehler>> {
        let tokens = self.oauth_tokens(Dienst::Atlassian)?;
        let (id, _) = self.atlassian_site()?;
        Some(
            jira_basis(&self.inner.quellen.endpunkte, &id)
                .and_then(|basis| JiraClient::oauth(&basis, tokens.access)),
        )
    }

    /// Verbindungsstatus eines Dienstes für `/api/health`. Blockierend.
    pub(crate) fn dienst_status(&self, d: Dienst, token_vorhanden: bool) -> ServiceStatus {
        let oauth_moeglich =
            self.oauth_konfig_id(d).is_some() && self.oauth_client_secret(d).is_some();
        let oauth_verbunden = self.oauth_tokens(d).is_some()
            && (d == Dienst::GitHub || self.atlassian_site().is_some());
        let (verbunden, art) = if oauth_verbunden {
            (true, AuthKind::OAuth)
        } else if token_vorhanden {
            (true, AuthKind::Token)
        } else {
            (false, AuthKind::None)
        };
        ServiceStatus {
            verbunden,
            art,
            oauth_moeglich,
            site: (d == Dienst::Atlassian && oauth_verbunden)
                .then(|| self.atlassian_site().map(|(_, n)| n))
                .flatten()
                .filter(|n| !n.is_empty()),
        }
    }

    // ───────────── Ablauf ─────────────

    /// Beginnt eine Anmeldung und liefert die Adresse des Dienstes.
    pub async fn oauth_start(&self, d: Dienst) -> Result<String, String> {
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
            .beginnen(d, jetzt_ms())
            .map_err(|e| e.to_string())?;
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        oauth::anmelde_url(&self.inner.quellen.endpunkte, d, &app, &state, &challenge)
            .map_err(|e| e.to_string())
    }

    /// Verarbeitet den Callback. `Ok`: eine Erfolgsmeldung, `Err`: eine verständliche Meldung.
    pub async fn oauth_callback(
        &self,
        d: Dienst,
        q: &HashMap<String, String>,
    ) -> Result<String, String> {
        let get = |k: &str| q.get(k).map(String::as_str).filter(|v| !v.is_empty());
        let state = get("state");
        // Der Nutzer hat abgelehnt oder der Dienst meldet einen Fehler: `state` verbrauchen.
        if let Some(fehler) = get("error") {
            if let (Some(st), Ok(mut a)) = (state, self.inner.anmeldungen.lock()) {
                let _ = a.entnehmen(st, d, jetzt_ms());
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
            .entnehmen(state, d, jetzt_ms())
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
        let e = &self.inner.quellen.endpunkte;
        let tokens = oauth::code_tauschen(e, d, &app, code, &verifier, jetzt_s())
            .await
            .map_err(|f| {
                tracing::warn!("{}: Austausch fehlgeschlagen: {f}", d.kennung());
                dienst_fehler(d, &f)
            })?;

        // Atlassian: erst die Site, dann wird gespeichert – kein halber Zustand.
        let site: Option<Site> = if d == Dienst::Atlassian {
            let gewuenscht = self
                .inner
                .quellen
                .atlassian
                .as_ref()
                .and_then(|a| a.site.clone());
            let liste = oauth::sites(e, &tokens.access).await.map_err(|f| {
                tracing::warn!("Atlassian: accessible-resources fehlgeschlagen: {f}");
                dienst_fehler(d, &f)
            })?;
            Some(site_waehlen(&liste, gewuenscht.as_deref()).map_err(|f| f.to_string())?)
        } else {
            None
        };

        let s = self.clone();
        let name = site.as_ref().map(|x| x.name.clone());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            s.oauth_tokens_setzen(d, &tokens).map_err(|e| {
                format!("Die Tokens ließen sich nicht im Schlüsselbund ablegen: {e}")
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
        tracing::info!("{}: angemeldet", d.kennung());

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
    /// im Schlüsselbund. Gelöscht wird immer – auch wenn der Dienst nicht antwortet.
    pub async fn oauth_widerrufen(&self, d: Dienst) -> Result<WiderrufAntwort, String> {
        let s = self.clone();
        let (tokens, secret) =
            tokio::task::spawn_blocking(move || (s.oauth_tokens(d), s.oauth_client_secret(d)))
                .await
                .map_err(|_| "Interner Fehler".to_string())?;
        let mut antwort = WiderrufAntwort::default();
        if let (Some(tokens), Some(secret), Some(client_id)) =
            (&tokens, &secret, self.oauth_konfig_id(d))
        {
            let uri = redirect_uri(d, self.inner.port);
            let app = App {
                client_id: &client_id,
                client_secret: secret,
                redirect_uri: &uri,
            };
            let api = self
                .inner
                .quellen
                .github
                .as_ref()
                .map_or(crate::config::STANDARD_GITHUB_API, |g| g.api_url.as_str());
            match oauth::widerrufen(d, &app, api, tokens).await {
                Ok(beim_dienst) => antwort.beim_dienst = beim_dienst,
                Err(f) => {
                    tracing::warn!("{}: Widerruf beim Dienst fehlgeschlagen: {f}", d.kennung());
                    antwort.hinweis = Some(format!(
                        "Lokal getrennt, aber der Dienst hat den Widerruf nicht bestätigt ({f}). Zugriff bitte in den Einstellungen bei {} entziehen.",
                        d.name()
                    ));
                }
            }
        }
        if d == Dienst::Atlassian && antwort.hinweis.is_none() {
            antwort.hinweis = Some(
                "Lokal getrennt. Atlassian bietet keinen Widerruf per Aufruf – den Zugriff entziehst du unter id.atlassian.com/manage-profile/apps."
                    .into(),
            );
        }
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.oauth_tokens_loeschen(d))
            .await
            .map_err(|_| "Interner Fehler".to_string())?;
        antwort.getrennt = true;
        tracing::info!("{}: getrennt", d.kennung());
        Ok(antwort)
    }

    /// Frischt Tokens auf, die bald ablaufen. Scheitert das, weil der Dienst das Refresh-Token
    /// nicht mehr gelten lässt, werden die Tokens gelöscht: Der Dienst gilt als getrennt.
    /// Netzwerkfehler und Störungen des Dienstes lassen alles, wie es ist.
    pub async fn oauth_auffrischen(&self) {
        for d in [Dienst::GitHub, Dienst::Atlassian] {
            if self.oauth_konfig_id(d).is_some() {
                self.oauth_auffrischen_fuer(d).await;
            }
        }
    }

    async fn oauth_auffrischen_fuer(&self, d: Dienst) {
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
        if !tokens.braucht_auffrischen(jetzt_s()) {
            return;
        }
        let (Some(secret), Some(client_id)) = (secret, self.oauth_konfig_id(d)) else {
            tracing::warn!(
                "{}: Client-Secret fehlt, Tokens nicht aufgefrischt",
                d.kennung()
            );
            return;
        };
        let uri = redirect_uri(d, self.inner.port);
        let app = App {
            client_id: &client_id,
            client_secret: &secret,
            redirect_uri: &uri,
        };
        let ergebnis =
            oauth::auffrischen(&self.inner.quellen.endpunkte, d, &app, &tokens, jetzt_s()).await;
        let s = self.clone();
        match ergebnis {
            Ok(neu) => {
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = s.oauth_tokens_setzen(d, &neu) {
                        tracing::warn!("{}: neue Tokens nicht ablegbar: {e}", d.kennung());
                    }
                })
                .await;
            }
            Err(
                f @ (Fehler::NichtAngemeldet
                | Fehler::Abgelehnt(_)
                | Fehler::KeineBerechtigung
                | Fehler::Antwort(_)),
            ) => {
                tracing::warn!("{}: Auffrischen abgelehnt ({f}) – getrennt", d.kennung());
                let _ = tokio::task::spawn_blocking(move || s.oauth_tokens_loeschen(d)).await;
            }
            Err(f) => tracing::warn!("{}: Auffrischen nicht möglich: {f}", d.kennung()),
        }
    }
}

// ───────────── Routen ─────────────

fn dienst_aus(kennung: &str) -> Result<Dienst, ()> {
    Dienst::von_kennung(kennung).ok_or(())
}

fn unbekannter_dienst() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            fehler: "Unbekannter Dienst".into(),
        }),
    )
        .into_response()
}

/// Zurück in die App – über die Adresse `127.0.0.1`, damit der gespeicherte Modus des Browsers
/// gilt, auch wenn der Dienst auf `localhost` zurückgeleitet hat.
fn zurueck(port: u16, d: Dienst, ergebnis: &Result<String, String>) -> Redirect {
    let basis = format!("http://127.0.0.1:{port}/");
    let ziel = match ergebnis {
        Ok(m) => format!(
            "{basis}?oauth=ok&dienst={}&meldung={}#/einrichten",
            d.kennung(),
            kodiert(m)
        ),
        Err(m) => format!(
            "{basis}?oauth=fehler&dienst={}&meldung={}#/einrichten",
            d.kennung(),
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
        tracing::warn!("{}: Anmeldung gescheitert: {m}", d.kennung());
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
            Json(ErrorBody { fehler: m }),
        )
            .into_response(),
    }
}

/// Zustand für `AppState`: offene Anmeldungen.
pub fn neue_anmeldungen() -> std::sync::Mutex<Anmeldungen> {
    std::sync::Mutex::new(Anmeldungen::default())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::Request;
    use cockpit_integrations::oauth::Endpunkte;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::abgleich::Quellen;
    use crate::app::router;
    use crate::config::{AtlassianKonfig, GitHubKonfig, JiraKonfig, OAuthKonfig};
    use crate::secrets::{MemoryStore, SecretStore};

    const CLIENT_GEHEIM: &str = "client-geheimnis-abc123";

    fn endpunkte(s: &MockServer) -> Endpunkte {
        Endpunkte {
            github_anmeldung: format!("{}/login/oauth/authorize", s.uri()),
            github_token: format!("{}/login/oauth/access_token", s.uri()),
            atlassian_anmeldung: format!("{}/authorize", s.uri()),
            atlassian_token: format!("{}/oauth/token", s.uri()),
            atlassian_api: s.uri(),
        }
    }

    struct Prufstand {
        state: AppState,
        app: Router,
        secrets: Arc<MemoryStore>,
    }

    fn stand(server: &MockServer, site: Option<&str>, mit_secret: bool) -> Prufstand {
        let secrets = Arc::new(MemoryStore::new());
        if mit_secret {
            for n in [secrets::GITHUB_OAUTH, secrets::ATLASSIAN_OAUTH] {
                secrets.set(n, &Secret::new(CLIENT_GEHEIM)).unwrap();
            }
        }
        let quellen = Quellen {
            github: Some(GitHubKonfig {
                api_url: server.uri(),
                repos: vec!["a/x".into()],
            }),
            jira: Some(JiraKonfig {
                site: String::new(),
                email: String::new(),
                projects: vec![],
                jql: None,
            }),
            github_oauth: Some(OAuthKonfig {
                client_id: "gh-id".into(),
            }),
            atlassian: Some(AtlassianKonfig {
                client_id: "atl-id".into(),
                site: site.map(Into::into),
            }),
            endpunkte: endpunkte(server),
            ..Quellen::default()
        };
        let state = AppState::mit_quellen(
            crate::db::open_im_speicher().unwrap(),
            secrets.clone(),
            8787,
            quellen,
        );
        Prufstand {
            app: router(state.clone(), PathBuf::from("/gibt/es/nicht")),
            state,
            secrets,
        }
    }

    async fn aufruf(app: &Router, methode: &str, pfad: &str) -> (StatusCode, String, String) {
        // Wie die Navigation des Browsers: kein Content-Type, kein Origin.
        let req = Request::builder()
            .method(methode)
            .uri(pfad)
            .header("host", "127.0.0.1:8787")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap();
        let r = app.clone().oneshot(req).await.unwrap();
        let status = r.status();
        let ort = r
            .headers()
            .get("location")
            .and_then(|l| l.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = r.into_body().collect().await.unwrap().to_bytes();
        (status, ort, String::from_utf8_lossy(&body).into_owned())
    }

    /// Ein Wert aus der Abfrage einer Adresse (prozentdekodiert).
    fn param(url: &str, name: &str) -> String {
        let abfrage = url
            .split_once('?')
            .map_or("", |(_, q)| q.split('#').next().unwrap_or(""));
        abfrage
            .split('&')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| entkodiert(v))
            .unwrap_or_default()
    }

    fn entkodiert(v: &str) -> String {
        let b = v.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'%' if i + 2 < b.len() => {
                    let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                    out.push(u8::from_str_radix(h, 16).unwrap_or(b'?'));
                    i += 3;
                }
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    async fn token_mock_github(server: &MockServer, erwartet: u64) {
        Mock::given(method("POST"))
            .and(path("/login/oauth/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ghu_zugriff_geheim", "expires_in": 28800,
                "refresh_token": "ghr_erneuern_geheim"
            })))
            .expect(erwartet)
            .mount(server)
            .await;
    }

    async fn github_state(p: &Prufstand) -> String {
        let (status, ort, _) = aufruf(&p.app, "GET", "/api/oauth/github/start").await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert!(ort.contains("/login/oauth/authorize?"), "{ort}");
        assert_eq!(param(&ort, "client_id"), "gh-id");
        assert_eq!(param(&ort, "code_challenge_method"), "S256");
        assert!(!param(&ort, "code_challenge").is_empty());
        assert_eq!(
            param(&ort, "redirect_uri"),
            "http://127.0.0.1:8787/api/oauth/github/callback"
        );
        assert!(!ort.contains(CLIENT_GEHEIM));
        param(&ort, "state")
    }

    #[tokio::test]
    async fn start_ohne_einrichtung_zeigt_eine_verstaendliche_meldung() {
        let server = MockServer::start().await;
        // Ohne Client-Secret im Schlüsselbund.
        let p = stand(&server, None, false);
        let (status, ort, _) = aufruf(&p.app, "GET", "/api/oauth/github/start").await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert!(
            ort.starts_with("http://127.0.0.1:8787/?oauth=fehler&dienst=github"),
            "{ort}"
        );
        let meldung = param(&ort, "meldung");
        assert!(meldung.contains("token setzen github-oauth"), "{meldung}");

        // Ohne Client-ID in der Konfiguration.
        let p = Prufstand {
            state: AppState::new(
                crate::db::open_im_speicher().unwrap(),
                Arc::new(MemoryStore::new()),
            ),
            app: Router::new(),
            secrets: Arc::new(MemoryStore::new()),
        };
        let _ = &p.secrets;
        assert!(p.state.oauth_start(Dienst::Atlassian).await.is_err());

        // Unbekannter Dienst.
        let p = stand(&server, None, true);
        let (status, _, _) = aufruf(&p.app, "GET", "/api/oauth/gitlab/start").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn github_anmeldung_von_start_bis_health() {
        let server = MockServer::start().await;
        token_mock_github(&server, 1).await;
        let p = stand(&server, None, true);
        let h = aufruf(&p.app, "GET", "/api/health").await.2;
        let h: serde_json::Value = serde_json::from_str(&h).unwrap();
        assert_eq!(h["anmeldung"]["github"]["verbunden"], false);
        assert_eq!(h["anmeldung"]["github"]["oauth_moeglich"], true);

        let state = github_state(&p).await;
        let (status, ort, body) = aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/github/callback?code=abc&state={state}&installation_id=5&setup_action=install"),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert!(
            ort.starts_with("http://127.0.0.1:8787/?oauth=ok&dienst=github"),
            "{ort}"
        );
        assert!(ort.ends_with("#/einrichten"));
        // Nichts Geheimes in Weiterleitung oder Body.
        for text in [&ort, &body] {
            for geheim in ["ghu_zugriff_geheim", "ghr_erneuern_geheim", CLIENT_GEHEIM] {
                assert!(!text.contains(geheim));
            }
        }
        // Tokens liegen im Schlüsselbund.
        let gespeichert = p
            .secrets
            .get(secrets::GITHUB_OAUTH_TOKENS)
            .unwrap()
            .unwrap();
        let t = Tokens::aus_text(gespeichert.expose()).unwrap();
        assert_eq!(t.access.expose(), "ghu_zugriff_geheim");
        assert_eq!(t.refresh.unwrap().expose(), "ghr_erneuern_geheim");

        let h = aufruf(&p.app, "GET", "/api/health").await.2;
        let h: serde_json::Value = serde_json::from_str(&h).unwrap();
        assert_eq!(h["anmeldung"]["github"]["verbunden"], true);
        assert_eq!(h["anmeldung"]["github"]["art"], "oauth");
        assert_eq!(h["verbindungen"]["github"], true);
        assert!(!h.to_string().contains("geheim"));
    }

    #[tokio::test]
    async fn falscher_und_doppelter_state() {
        let server = MockServer::start().await;
        token_mock_github(&server, 1).await; // genau ein Austausch
        let p = stand(&server, None, true);

        // Falscher state: kein Austausch, nichts gespeichert.
        let _ = github_state(&p).await;
        let (_, ort, _) = aufruf(
            &p.app,
            "GET",
            "/api/oauth/github/callback?code=abc&state=falsch",
        )
        .await;
        assert!(ort.contains("oauth=fehler"), "{ort}");
        assert!(param(&ort, "meldung").contains("unbekannt"));
        // Ohne state (z. B. Rücksprung der Installation ohne unseren Start).
        let (_, ort, _) = aufruf(
            &p.app,
            "GET",
            "/api/oauth/github/callback?code=abc&installation_id=5&setup_action=install",
        )
        .await;
        assert!(ort.contains("oauth=fehler"));
        assert!(param(&ort, "meldung").contains("Mit GitHub anmelden"));
        assert!(
            p.secrets
                .get(secrets::GITHUB_OAUTH_TOKENS)
                .unwrap()
                .is_none()
        );

        // Richtig, dann derselbe Callback noch einmal.
        let state = github_state(&p).await;
        let url = format!("/api/oauth/github/callback?code=abc&state={state}");
        assert!(aufruf(&p.app, "GET", &url).await.1.contains("oauth=ok"));
        let (_, ort, _) = aufruf(&p.app, "GET", &url).await;
        assert!(ort.contains("oauth=fehler"), "{ort}");
        assert!(param(&ort, "meldung").contains("schon verwendet"));
        assert!(
            p.secrets
                .get(secrets::GITHUB_OAUTH_TOKENS)
                .unwrap()
                .is_some()
        );

        // State von GitHub, Callback von Atlassian.
        let state = github_state(&p).await;
        let (_, ort, _) = aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/atlassian/callback?code=abc&state={state}"),
        )
        .await;
        assert!(param(&ort, "meldung").contains("anderen Dienst"), "{ort}");
    }

    #[tokio::test]
    async fn fehler_des_dienstes_und_abbruch() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login/oauth/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "error": "bad_verification_code",
                "error_description": "Der Code ist abgelaufen"
            })))
            .mount(&server)
            .await;
        let p = stand(&server, None, true);
        let state = github_state(&p).await;
        let (_, ort, _) = aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/github/callback?code=abc&state={state}"),
        )
        .await;
        assert!(ort.contains("oauth=fehler"));
        assert!(param(&ort, "meldung").contains("abgelaufen"));
        assert!(
            p.secrets
                .get(secrets::GITHUB_OAUTH_TOKENS)
                .unwrap()
                .is_none()
        );

        // Der Nutzer lehnt ab.
        let state = github_state(&p).await;
        let (_, ort, _) = aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/github/callback?error=access_denied&error_description=The+user+denied&state={state}"),
        )
        .await;
        assert!(
            param(&ort, "meldung").contains("abgebrochen oder abgelehnt: The user denied"),
            "{ort}"
        );
        // Der state ist damit verbraucht.
        let (_, ort, _) = aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/github/callback?code=abc&state={state}"),
        )
        .await;
        assert!(param(&ort, "meldung").contains("schon verwendet"));
    }

    async fn atlassian_mocks(server: &MockServer, sites: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "atl_zugriff_geheim", "expires_in": 3600,
                "refresh_token": "atl_erneuern_geheim"
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/oauth/token/accessible-resources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sites))
            .mount(server)
            .await;
    }

    async fn atlassian_anmelden(p: &Prufstand) -> String {
        let (_, ort, _) = aufruf(&p.app, "GET", "/api/oauth/atlassian/start").await;
        assert!(ort.contains("/authorize?"), "{ort}");
        assert_eq!(param(&ort, "audience"), "api.atlassian.com");
        assert!(param(&ort, "scope").contains("offline_access"));
        assert_eq!(
            param(&ort, "redirect_uri"),
            "http://localhost:8787/api/oauth/atlassian/callback"
        );
        let state = param(&ort, "state");
        aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/atlassian/callback?code=c&state={state}"),
        )
        .await
        .1
    }

    #[tokio::test]
    async fn atlassian_eine_site_wird_automatisch_gewaehlt() {
        let server = MockServer::start().await;
        atlassian_mocks(
            &server,
            serde_json::json!([{"id":"cloud-1","name":"firma","url":"https://firma.atlassian.net","scopes":[]}]),
        )
        .await;
        let p = stand(&server, None, true);
        let ort = atlassian_anmelden(&p).await;
        assert!(
            ort.starts_with("http://127.0.0.1:8787/?oauth=ok&dienst=atlassian"),
            "{ort}"
        );
        assert!(param(&ort, "meldung").contains("firma"));
        assert_eq!(
            p.state.atlassian_site(),
            Some(("cloud-1".into(), "firma".into()))
        );
        let h = aufruf(&p.app, "GET", "/api/health").await.2;
        let h: serde_json::Value = serde_json::from_str(&h).unwrap();
        assert_eq!(h["anmeldung"]["jira"]["art"], "oauth");
        assert_eq!(h["anmeldung"]["jira"]["site"], "firma");
        assert_eq!(h["verbindungen"]["jira"], true);
        // Jira wird über das Gateway angesprochen.
        let dienste = tokio::task::spawn_blocking({
            let s = p.state.clone();
            move || s.dienste_bauen()
        })
        .await
        .unwrap();
        assert!(dienste.jira.is_some());
    }

    #[tokio::test]
    async fn atlassian_mehrere_sites_brauchen_eine_wahl() {
        let server = MockServer::start().await;
        atlassian_mocks(
            &server,
            serde_json::json!([
                {"id":"c1","name":"firma","url":"https://firma.atlassian.net","scopes":[]},
                {"id":"c2","name":"privat","url":"https://privat.atlassian.net","scopes":[]}
            ]),
        )
        .await;
        let p = stand(&server, None, true);
        let ort = atlassian_anmelden(&p).await;
        assert!(ort.contains("oauth=fehler"), "{ort}");
        let meldung = param(&ort, "meldung");
        assert!(
            meldung.contains("firma") && meldung.contains("privat"),
            "{meldung}"
        );
        // Kein halber Zustand.
        assert!(
            p.secrets
                .get(secrets::ATLASSIAN_OAUTH_TOKENS)
                .unwrap()
                .is_none()
        );
        assert_eq!(p.state.atlassian_site(), None);

        let p = stand(&server, Some("privat"), true);
        let ort = atlassian_anmelden(&p).await;
        assert!(ort.contains("oauth=ok"), "{ort}");
        assert_eq!(p.state.atlassian_site().unwrap().0, "c2");
    }

    #[tokio::test]
    async fn widerruf_github_ruft_den_dienst_und_loescht() {
        let server = MockServer::start().await;
        token_mock_github(&server, 1).await;
        Mock::given(method("DELETE"))
            .and(path("/applications/gh-id/token"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let p = stand(&server, None, true);
        let state = github_state(&p).await;
        aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/github/callback?code=c&state={state}"),
        )
        .await;
        assert!(p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());

        let (status, _, body) = aufruf(&p.app, "POST", "/api/oauth/github/widerrufen").await;
        assert_eq!(status, StatusCode::OK);
        let a: WiderrufAntwort = serde_json::from_str(&body).unwrap();
        assert!(a.getrennt && a.beim_dienst, "{body}");
        assert!(!p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());
        // Das Client-Secret bleibt (für die nächste Anmeldung).
        assert!(p.secrets.has(secrets::GITHUB_OAUTH).unwrap());
        let h = aufruf(&p.app, "GET", "/api/health").await.2;
        let h: serde_json::Value = serde_json::from_str(&h).unwrap();
        assert_eq!(h["anmeldung"]["github"]["verbunden"], false);
        assert_eq!(h["anmeldung"]["github"]["art"], "none");
    }

    #[tokio::test]
    async fn widerruf_trennt_auch_wenn_der_dienst_scheitert() {
        let server = MockServer::start().await;
        token_mock_github(&server, 1).await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let p = stand(&server, None, true);
        let state = github_state(&p).await;
        aufruf(
            &p.app,
            "GET",
            &format!("/api/oauth/github/callback?code=c&state={state}"),
        )
        .await;
        let a = p.state.oauth_widerrufen(Dienst::GitHub).await.unwrap();
        assert!(a.getrennt && !a.beim_dienst);
        assert!(a.hinweis.unwrap().contains("nicht bestätigt"));
        assert!(!p.secrets.has(secrets::GITHUB_OAUTH_TOKENS).unwrap());
    }

    #[tokio::test]
    async fn widerruf_atlassian_loescht_tokens_und_site() {
        let server = MockServer::start().await;
        atlassian_mocks(
            &server,
            serde_json::json!([{"id":"c1","name":"firma","url":"https://firma.atlassian.net","scopes":[]}]),
        )
        .await;
        let p = stand(&server, None, true);
        atlassian_anmelden(&p).await;
        let (status, _, body) = aufruf(&p.app, "POST", "/api/oauth/atlassian/widerrufen").await;
        assert_eq!(status, StatusCode::OK);
        let a: WiderrufAntwort = serde_json::from_str(&body).unwrap();
        assert!(a.getrennt && !a.beim_dienst);
        assert!(a.hinweis.unwrap().contains("id.atlassian.com"));
        assert!(!p.secrets.has(secrets::ATLASSIAN_OAUTH_TOKENS).unwrap());
        assert_eq!(p.state.atlassian_site(), None);
        // Widerruf ohne Anmeldung ist kein Fehler.
        assert!(
            p.state
                .oauth_widerrufen(Dienst::Atlassian)
                .await
                .unwrap()
                .getrennt
        );
    }

    fn abgelaufen(p: &Prufstand, d: Dienst, refresh: Option<&str>) {
        let t = Tokens {
            access: Secret::new("alt_zugriff"),
            refresh: refresh.map(Secret::new),
            laeuft_ab_s: Some(1),
        };
        p.state.oauth_tokens_setzen(d, &t).unwrap();
    }

    #[tokio::test]
    async fn abgelaufene_tokens_werden_aufgefrischt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login/oauth/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "neu_zugriff", "expires_in": 28800, "refresh_token": "neu_erneuern"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let p = stand(&server, None, true);
        abgelaufen(&p, Dienst::GitHub, Some("alt_erneuern"));
        p.state.oauth_auffrischen().await;
        let t = p.state.oauth_tokens(Dienst::GitHub).unwrap();
        assert_eq!(t.access.expose(), "neu_zugriff");
        assert_eq!(t.refresh.unwrap().expose(), "neu_erneuern");
        // Noch gültig: kein zweiter Aufruf (`expect(1)`).
        p.state.oauth_auffrischen().await;
        // Der Client bekommt das neue Token.
        let d = p.state.dienste_bauen();
        assert!(d.github.is_some());
    }

    #[tokio::test]
    async fn scheitert_der_refresh_ist_der_dienst_getrennt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login/oauth/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "error": "bad_refresh_token", "error_description": "abgelaufen"
            })))
            .mount(&server)
            .await;
        let p = stand(&server, None, true);
        abgelaufen(&p, Dienst::GitHub, Some("alt_erneuern"));
        p.state.oauth_auffrischen().await;
        assert!(p.state.oauth_tokens(Dienst::GitHub).is_none());
        let s = p.state.dienst_status(Dienst::GitHub, false);
        assert!(!s.verbunden);
        assert_eq!(s.art, AuthKind::None);

        // Ohne Refresh-Token ist der abgelaufene Zugriff nicht zu retten.
        abgelaufen(&p, Dienst::GitHub, None);
        p.state.oauth_auffrischen().await;
        assert!(p.state.oauth_tokens(Dienst::GitHub).is_none());
    }

    #[tokio::test]
    async fn stoerung_des_dienstes_trennt_nicht() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let p = stand(&server, None, true);
        abgelaufen(&p, Dienst::GitHub, Some("alt_erneuern"));
        p.state.oauth_auffrischen().await;
        assert!(p.state.oauth_tokens(Dienst::GitHub).is_some());
    }

    #[tokio::test]
    async fn oauth_geht_vor_dem_api_token() {
        let server = MockServer::start().await;
        let p = stand(&server, None, true);
        p.secrets
            .set(secrets::GITHUB, &Secret::new("ghp_api_token_123"))
            .unwrap();
        let status = p.state.dienst_status(Dienst::GitHub, true);
        assert_eq!((status.verbunden, status.art), (true, AuthKind::Token));
        p.state
            .oauth_tokens_setzen(
                Dienst::GitHub,
                &Tokens {
                    access: Secret::new("ghu_x"),
                    refresh: None,
                    laeuft_ab_s: None,
                },
            )
            .unwrap();
        let status = p.state.dienst_status(Dienst::GitHub, true);
        assert_eq!(status.art, AuthKind::OAuth);
        assert!(status.oauth_moeglich);
        // Nach dem Trennen gilt wieder das Token.
        p.state.oauth_widerrufen(Dienst::GitHub).await.unwrap();
        assert_eq!(
            p.state.dienst_status(Dienst::GitHub, true).art,
            AuthKind::Token
        );
    }

    #[tokio::test]
    async fn callback_ist_eine_navigation_aber_nur_von_diesem_rechner() {
        let server = MockServer::start().await;
        let p = stand(&server, None, true);
        // Navigation des Browsers: GET ohne JSON-Content-Type und ohne Origin.
        let req = Request::builder()
            .uri("/api/oauth/github/callback?code=a&state=b")
            .header("host", "localhost:8787")
            .header("sec-fetch-mode", "navigate")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap();
        let r = p.app.clone().oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        // Fremder Host (DNS-Rebinding) und fremde Gegenstelle bleiben draußen.
        for (host, peer) in [
            ("evil.example:8787", [127, 0, 0, 1]),
            ("127.0.0.1:8787", [10, 0, 0, 5]),
        ] {
            let req = Request::builder()
                .uri("/api/oauth/github/callback?code=a&state=b")
                .header("host", host)
                .extension(ConnectInfo(SocketAddr::from((peer, 50000))))
                .body(Body::empty())
                .unwrap();
            let r = p.app.clone().oneshot(req).await.unwrap();
            assert_eq!(r.status(), StatusCode::FORBIDDEN, "{host}");
        }
    }

    #[test]
    fn adressen_und_kodierung() {
        assert_eq!(
            redirect_uri(Dienst::GitHub, 8787),
            "http://127.0.0.1:8787/api/oauth/github/callback"
        );
        assert_eq!(
            redirect_uri(Dienst::Atlassian, 8787),
            "http://localhost:8787/api/oauth/atlassian/callback"
        );
        assert_eq!(kodiert("Ä b&c=d#"), "%C3%84%20b%26c%3Dd%23");
    }
}
