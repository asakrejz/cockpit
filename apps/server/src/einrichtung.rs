//! Einrichtung per Klick: Routen für den Assistenten in der App.
//!
//! * `GET /api/einrichtung`: aktuelle, nicht geheime Werte und welche Schlüssel liegen
//! * `PUT /api/konfiguration`: schreibt Client-IDs, Jira, Repos sowie Agent-Anbieter und Endpunkt in `config.toml`
//!   (atomar, Kommentare und andere Werte bleiben) und wendet sie ohne Neustart an
//! * `PUT /api/schluessel/{name}`: legt einen Schlüssel im Schlüsselbund ab; `DELETE` löscht einen
//!   Token (`jira`, `github`) wieder („Trennen“)
//! * `POST /api/jira/pruefen`: Verbindungstest für Jira per API-Token (nur Lesen)
//!
//! Alle drei laufen hinter der Prüfung aus `zugang` (Loopback, Host, Origin, Content-Type).
//! Ein Schlüssel kommt nur im Body der Anfrage vor, wird nirgends geloggt und nie zurückgeschickt;
//! die Antwort sagt nur „gesetzt“.

use std::io::Write;
use std::path::Path;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as UrlPfad, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cockpit_core::api::{
    ConfigChange, ConfigResponse, JiraCause, JiraCheck, KeyInput, KeyResponse, Setup,
};
use cockpit_core::setup::{
    KEY_NAMES, client_id_read, endpoint_url_read, jira_check_text, jira_site_read, key_deletable,
    key_name_allowed, key_verify, projects_read, repos_read,
};
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::Service;
use cockpit_integrations::{Error, Secret};
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::app::{AppState, fehler};
use crate::config::{Config, KonfigFehler};
use crate::db;
use crate::oauth::redirect_uri;
use crate::secrets;

/// Setzt `schluessel = wert` in der Tabelle und behält Kommentare der Zeile (davor und dahinter).
fn setzen(tabelle: &mut dyn toml_edit::TableLike, schluessel: &str, neu: Item) {
    // Den vorhandenen Eintrag an Ort und Stelle ersetzen: So bleibt der Schlüssel samt Kommentar davor.
    let Some(alt) = tabelle.get_mut(schluessel) else {
        tabelle.insert(schluessel, neu);
        return;
    };
    let decor = alt.as_value().map(|v| v.decor().clone());
    *alt = neu;
    if let (Some(decor), Some(v)) = (decor, alt.as_value_mut()) {
        *v.decor_mut() = decor;
    }
}

/// Die Tabelle `[name]`; sie wird angelegt, falls sie fehlt.
fn tabelle<'a>(
    doc: &'a mut DocumentMut,
    name: &str,
) -> Result<&'a mut dyn toml_edit::TableLike, String> {
    if !doc.contains_key(name) {
        doc.insert(name, Item::Table(Table::new()));
    }
    doc.get_mut(name)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| {
            format!("In config.toml ist „{name}“ keine Tabelle – bitte die Datei prüfen.")
        })
}

/// Entfernt eine Tabelle, wenn sie leer ist.
fn leer_entfernen(doc: &mut DocumentMut, name: &str) {
    let leer = doc
        .get(name)
        .and_then(Item::as_table_like)
        .is_some_and(|t| t.is_empty());
    if leer {
        doc.remove(name);
    }
}

fn liste(eintraege: &[String]) -> Item {
    value(eintraege.iter().map(String::as_str).collect::<Array>())
}

/// Schreibt Anbieter und Endpunkt im neuen Format (`[agent] anbieter`, `[agent.endpunkt]`). Ein
/// älteres `[litellm]` wird dabei übernommen und entfernt, damit nur ein Ort gilt.
fn endpunkt_schreiben(
    doc: &mut DocumentMut,
    e: &cockpit_core::api::EndpointInput,
) -> Result<(), String> {
    let url = e.url.as_deref().map(endpoint_url_read).transpose()?;
    let modell = match e.model.as_deref().map(str::trim) {
        Some(m) if m.chars().any(|c| c.is_whitespace() || c.is_control()) => {
            return Err("Der Modellname enthält Leerzeichen oder Sonderzeichen.".into());
        }
        anderes => anderes,
    };
    // Werte aus einem älteren `[litellm]` retten, bevor es verschwindet.
    let alt: Option<(Option<String>, Option<String>)> =
        doc.get("litellm").and_then(Item::as_table_like).map(|t| {
            let text = |k: &str| t.get(k).and_then(Item::as_str).map(str::to_string);
            (text("url"), text("modell"))
        });
    if alt.is_some() {
        doc.remove("litellm");
    }
    {
        let agent = tabelle(doc, "agent")?;
        if let Some(anbieter) = e.provider {
            setzen(agent, "anbieter", value(anbieter.id()));
        }
        if !agent.contains_key("endpunkt") {
            agent.insert("endpunkt", Item::Table(Table::new()));
        }
        let ep = agent
            .get_mut("endpunkt")
            .and_then(Item::as_table_like_mut)
            .ok_or("In config.toml ist „agent.endpunkt“ keine Tabelle – bitte die Datei prüfen.")?;
        if let Some((alte_url, altes_modell)) = alt {
            if !ep.contains_key("url")
                && let Some(u) = alte_url
            {
                ep.insert("url", value(u));
            }
            if !ep.contains_key("modell")
                && let Some(m) = altes_modell
            {
                ep.insert("modell", value(m));
            }
        }
        match url.as_deref() {
            Some("") => {
                ep.remove("url");
            }
            Some(u) => setzen(ep, "url", value(u)),
            None => {}
        }
        match modell {
            Some("") => {
                ep.remove("modell");
            }
            Some(m) => setzen(ep, "modell", value(m)),
            None => {}
        }
        if ep.is_empty() {
            agent.remove("endpunkt");
        }
    }
    leer_entfernen(doc, "agent");
    Ok(())
}

/// Wendet die Änderung auf den Text von `config.toml` an. Der Text wird anschließend wie beim
/// Start geprüft (`Config::aus`); ein Fehler heißt: nichts schreiben.
pub fn aendern(text: &str, a: &ConfigChange) -> Result<(String, Config), String> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        format!("config.toml ist fehlerhaft: {}", e.message())
    })?;

    if let Some(j) = &a.jira {
        {
            let t = tabelle(&mut doc, "jira")?;
            if let Some(site) = &j.site {
                let site = jira_site_read(site)?;
                if site.is_empty() {
                    t.remove("site");
                } else {
                    setzen(t, "site", value(site));
                }
            }
            if let Some(email) = &j.email {
                let email = email.trim();
                if email.is_empty() {
                    t.remove("email");
                } else if !email.contains('@') || email.contains(char::is_whitespace) {
                    return Err("Das ist keine E-Mail-Adresse.".into());
                } else {
                    setzen(t, "email", value(email));
                }
            }
            if let Some(p) = &j.projects {
                let projekte = projects_read(&p.join(","));
                if projekte.is_empty() {
                    t.remove("projects");
                } else {
                    setzen(t, "projects", liste(&projekte));
                }
            }
        }
        leer_entfernen(&mut doc, "jira");
    }
    for (name, eingabe) in [
        ("atlassian", &a.atlassian),
        ("github_oauth", &a.github_oauth),
    ] {
        if let Some(c) = eingabe {
            let id = client_id_read(&c.client_id)?;
            if id.is_empty() {
                doc.remove(name);
            } else {
                setzen(tabelle(&mut doc, name)?, "client_id", value(id));
            }
        }
    }
    if let Some(g) = &a.github {
        let repos = repos_read(&g.repos.join(","))?;
        {
            let t = tabelle(&mut doc, "github")?;
            if repos.is_empty() {
                t.remove("repos");
            } else {
                setzen(t, "repos", liste(&repos));
            }
        }
        leer_entfernen(&mut doc, "github");
    }
    if let Some(e) = &a.endpoint {
        endpunkt_schreiben(&mut doc, e)?;
    }

    let neu = doc.to_string();
    let config = Config::aus(Some(&neu), |_| None).map_err(|e| match e {
        KonfigFehler::Wert(m) => m,
        andere => andere.to_string(),
    })?;
    Ok((neu, config))
}

/// Schreibt die Datei atomar: erst eine Nachbardatei, dann umbenennen. Eine vorhandene Datei
/// behält ihre Rechte; eine neue ist nur für den Nutzer lesbar.
pub fn atomar_schreiben(pfad: &Path, text: &str) -> std::io::Result<()> {
    // Ein Link auf die Datei bleibt ein Link: Wir ersetzen das Ziel.
    let pfad = std::fs::canonicalize(pfad).unwrap_or_else(|_| pfad.to_path_buf());
    let ordner = pfad
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(ordner)?;
    let name = pfad
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".into());
    let tmp = ordner.join(format!(".{name}.neu-{}", std::process::id()));
    let ergebnis = (|| {
        let mut datei = std::fs::File::create(&tmp)?;
        if let Ok(alt) = std::fs::metadata(&pfad) {
            std::fs::set_permissions(&tmp, alt.permissions())?;
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        datei.write_all(text.as_bytes())?;
        datei.sync_all()?;
        std::fs::rename(&tmp, &pfad)
    })();
    if ergebnis.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    ergebnis
}

impl AppState {
    /// Übernimmt die Adressen und Konten aus der Konfiguration in die Datenbank (ohne Schlüssel).
    /// Blockierend.
    pub(crate) fn verbindungen_uebernehmen(&self, config: &Config) -> Result<(), String> {
        let conn = self
            .inner
            .db
            .lock()
            .map_err(|_| "Datenbank gesperrt".to_string())?;
        if let Some(g) = &config.github {
            db::verbindung_setzen(&conn, secrets::GITHUB, &g.api_url, None)
                .map_err(|e| e.to_string())?;
        }
        // Mit OAuth hat Jira keine eigene Adresse; die Site wählt die Anmeldung.
        if let Some(j) = config.jira.as_ref().filter(|j| !j.site.is_empty()) {
            db::verbindung_setzen(&conn, secrets::JIRA, &j.site, Some(&j.email))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Ändert `config.toml` und wendet das Ergebnis an. Blockierend.
    fn konfiguration_aendern(
        &self,
        a: &ConfigChange,
    ) -> Result<ConfigResponse, (StatusCode, String)> {
        let Some(pfad) = self.inner.einstellungen.konfig_pfad.clone() else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Dieser Rechner hat keinen Ort für die Konfigurationsdatei. Setze COCKPIT_CONFIG auf einen Dateipfad."
                    .into(),
            ));
        };
        let _sperre = self.inner.konfig_sperre.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Interner Fehler".to_string(),
            )
        })?;
        let alt = match std::fs::read_to_string(&pfad) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                tracing::warn!("config.toml nicht lesbar: {e}");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Die Datei {} lässt sich nicht lesen.", pfad.display()),
                ));
            }
        };
        let (neu, config) = aendern(&alt, a).map_err(|m| (StatusCode::UNPROCESSABLE_ENTITY, m))?;
        let vorher = self.quellen();
        if neu != alt {
            atomar_schreiben(&pfad, &neu).map_err(|e| {
                tracing::warn!("config.toml nicht geschrieben: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Die Datei {} lässt sich nicht schreiben ({}).",
                        pfad.display(),
                        e.kind()
                    ),
                )
            })?;
        }
        self.verbindungen_uebernehmen(&config)
            .map_err(|m| (StatusCode::INTERNAL_SERVER_ERROR, m))?;
        self.konfig_anwenden(&config);
        tracing::info!("config.toml aktualisiert und angewendet");

        // Eine andere App heißt: Die alten Tokens gehören zur alten App.
        let andere_app = |alt: Option<&String>, neu: Option<&String>| {
            alt.is_some() && neu.is_some() && alt != neu
        };
        let id_alt_g = vorher.github_oauth.as_ref().map(|o| &o.client_id);
        let id_neu_g = config.github_oauth.as_ref().map(|o| &o.client_id);
        let id_alt_a = vorher.atlassian.as_ref().map(|o| &o.client_id);
        let id_neu_a = config.atlassian.as_ref().map(|o| &o.client_id);
        let hinweis = (andere_app(id_alt_g, id_neu_g) || andere_app(id_alt_a, id_neu_a)).then(|| {
            "Du hast die Client-ID einer App geändert. Melde dich danach neu an, damit die Anmeldung zur neuen App gehört."
                .to_string()
        });
        Ok(ConfigResponse {
            stored: true,
            takes_effect_immediately: true,
            hint: hinweis,
        })
    }

    /// Die aktuellen, nicht geheimen Werte. Blockierend (Schlüsselbund).
    fn einrichtung(&self) -> Setup {
        let q = self.quellen();
        let wahl = self.agent_wahl();
        let port = self.inner.port;
        Setup {
            jira_site: q.jira.as_ref().map(|j| j.site.clone()).unwrap_or_default(),
            jira_email: q.jira.as_ref().map(|j| j.email.clone()).unwrap_or_default(),
            jira_projects: q
                .jira
                .as_ref()
                .map(|j| j.projects.clone())
                .unwrap_or_default(),
            atlassian_client_id: q.atlassian.as_ref().map(|a| a.client_id.clone()),
            github_client_id: q.github_oauth.as_ref().map(|o| o.client_id.clone()),
            github_repos: q
                .github
                .as_ref()
                .map(|g| g.repos.clone())
                .unwrap_or_default(),
            provider: wahl.anbieter,
            endpoint_url: wahl.endpunkt.as_ref().and_then(|e| e.url.clone()),
            endpoint_model: wahl.endpunkt.and_then(|e| e.modell),
            callback_github: redirect_uri(Service::GitHub, port),
            callback_atlassian: redirect_uri(Service::Atlassian, port),
            key: KEY_NAMES
                .iter()
                .map(|n| KeyResponse {
                    name: (*n).to_string(),
                    set: self.inner.secrets.has(n).unwrap_or(false),
                })
                .collect(),
        }
    }
}

/// `GET /api/einrichtung`.
pub async fn lesen(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.einrichtung()).await {
        Ok(e) => Json(e).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `PUT /api/konfiguration`.
pub async fn konfiguration(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(a) = serde_json::from_slice::<ConfigChange>(&body) else {
        return fehler(StatusCode::BAD_REQUEST, "Die Eingaben sind nicht lesbar");
    };
    match tokio::task::spawn_blocking(move || state.konfiguration_aendern(&a)).await {
        Ok(Ok(antwort)) => Json(antwort).into_response(),
        Ok(Err((status, meldung))) => fehler(status, meldung),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `PUT /api/schluessel/{name}`: legt den Schlüssel im Schlüsselbund ab. Der Wert steht nur im
/// Body; er wird weder geloggt noch zurückgeschickt, und Fehlermeldungen nennen ihn nie.
pub async fn schluessel(
    State(state): State<AppState>,
    UrlPfad(name): UrlPfad<String>,
    body: Bytes,
) -> Response {
    if !key_name_allowed(&name) {
        return fehler(
            StatusCode::NOT_FOUND,
            format!("Einen Schlüssel „{name}“ gibt es nicht"),
        );
    }
    let Ok(eingabe) = serde_json::from_slice::<KeyInput>(&body) else {
        return fehler(StatusCode::BAD_REQUEST, "Die Eingabe ist nicht lesbar");
    };
    let wert = match key_verify(&eingabe.value) {
        Ok(w) => w,
        Err(m) => return fehler(StatusCode::UNPROCESSABLE_ENTITY, m),
    };
    let secrets = state.inner.secrets.clone();
    let n = name.clone();
    let ergebnis = tokio::task::spawn_blocking(move || secrets.set(&n, &Secret::new(wert))).await;
    match ergebnis {
        Ok(Ok(())) => {
            tracing::info!("Schlüssel für {name} abgelegt");
            Json(KeyResponse { name, set: true }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Schlüssel für {name} nicht abgelegt: {e}");
            fehler(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Schlüsselbund hat den Schlüssel nicht angenommen. Prüfe, ob der Schlüsselbund entsperrt ist.",
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `DELETE /api/schluessel/{name}`: löscht einen Token aus dem Schlüsselbund („Trennen“). Erlaubt
/// sind nur die Tokens `jira` und `github`; ein nicht vorhandener Schlüssel ist kein Fehler.
pub async fn schluessel_loeschen(
    State(state): State<AppState>,
    UrlPfad(name): UrlPfad<String>,
) -> Response {
    if !key_deletable(&name) {
        return fehler(
            StatusCode::NOT_FOUND,
            format!("Einen löschbaren Schlüssel „{name}“ gibt es nicht"),
        );
    }
    let secrets = state.inner.secrets.clone();
    let n = name.clone();
    match tokio::task::spawn_blocking(move || secrets.delete(&n)).await {
        Ok(Ok(())) => {
            tracing::info!("Schlüssel für {name} gelöscht");
            Json(KeyResponse { name, set: false }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Schlüssel für {name} nicht gelöscht: {e}");
            fehler(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Schlüsselbund hat den Schlüssel nicht gelöscht. Prüfe, ob der Schlüsselbund entsperrt ist.",
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn pruefung(ursache: JiraCause, name: Option<String>) -> JiraCheck {
    JiraCheck {
        ok: ursache == JiraCause::Ok,
        cause: ursache,
        message: jira_check_text(ursache, name.as_deref()),
        name: name.filter(|n| !n.is_empty()),
    }
}

/// Welche Ursache zu einem Fehler von Jira gehört.
fn jira_ursache(f: &Error) -> JiraCause {
    match f {
        Error::NotSignedIn => JiraCause::Token,
        Error::NoPermission => JiraCause::Permission,
        // 404, eine fremde Seite statt Jira (kein JSON) oder eine unbrauchbare Adresse.
        Error::NotFound | Error::Response(_) | Error::Config(_) => JiraCause::Address,
        Error::Network(_) => JiraCause::Reachable,
        _ => JiraCause::Other,
    }
}

impl AppState {
    /// Der Verbindungstest für Jira mit E-Mail und API-Token (nie mit OAuth): Adresse und E-Mail
    /// aus der Konfiguration, der Token aus dem Schlüsselbund. Nur `GET /rest/api/3/myself`.
    pub async fn jira_pruefen(&self) -> JiraCheck {
        let s = self.clone();
        let angaben = tokio::task::spawn_blocking(move || {
            let j = s.quellen().jira.clone().filter(|j| !j.site.is_empty());
            j.map(|j| (j.site, j.email, s.token(crate::secrets::JIRA)))
        })
        .await
        .ok()
        .flatten();
        let Some((site, email, Some(token))) = angaben.filter(|(_, e, _)| !e.is_empty()) else {
            return pruefung(JiraCause::Details, None);
        };
        let client = match JiraClient::new(&site, &email, token) {
            Ok(c) => c,
            Err(e) => return pruefung(jira_ursache(&e), None),
        };
        match client.my_name().await {
            Ok(name) => pruefung(JiraCause::Ok, Some(name)),
            Err(e) => {
                tracing::warn!("Jira-Verbindungstest: {e}");
                pruefung(jira_ursache(&e), None)
            }
        }
    }
}

/// `POST /api/jira/pruefen`: ohne Body; liest nur.
pub async fn jira_pruefen(State(state): State<AppState>) -> Response {
    Json(state.jira_pruefen().await).into_response()
}

#[cfg(test)]
mod tests;
