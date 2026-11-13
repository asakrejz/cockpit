//! Einrichtung per Klick: Routen für den Assistenten in der App.
//!
//! * `GET /api/setup`: aktuelle, nicht geheime Werte und welche Schlüssel liegen
//! * `PUT /api/config`: schreibt Client-IDs, Jira, Repos sowie Agent-Anbieter und Endpunkt in `config.toml`
//!   (atomar, Kommentare und andere Werte bleiben) und wendet sie ohne Neustart an
//! * `PUT /api/secrets/{name}`: legt einen Schlüssel im Schlüsselbund ab; `DELETE` löscht einen
//!   Token (`jira`, `github`) wieder („Trennen“)
//! * `POST /api/jira/check`: Verbindungstest für Jira per API-Token (nur Lesen)
//!
//! Alle drei laufen hinter der Prüfung aus `zugang` (Loopback, Host, Origin, Content-Type).
//! Ein Schlüssel kommt nur im Body der Anfrage vor, wird nirgends geloggt und nie zurückgeschickt;
//! die Antwort sagt nur „gesetzt“.

use std::io::Write;
use std::path::Path;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
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

use crate::app::{AppState, error};
use crate::config::{Config, ConfigError};
use crate::db;
use crate::oauth::redirect_uri;
use crate::secrets;

/// Setzt `schluessel = wert` in der Tabelle und behält Kommentare der Zeile (davor und dahinter).
fn set(table: &mut dyn toml_edit::TableLike, key: &str, updated: Item) {
    // Den vorhandenen Eintrag an Ort und Stelle ersetzen: So bleibt der Schlüssel samt Kommentar davor.
    let Some(alt) = table.get_mut(key) else {
        table.insert(key, updated);
        return;
    };
    let decor = alt.as_value().map(|v| v.decor().clone());
    *alt = updated;
    if let (Some(decor), Some(v)) = (decor, alt.as_value_mut()) {
        *v.decor_mut() = decor;
    }
}

/// Die Tabelle `[name]`; sie wird angelegt, falls sie fehlt.
fn table<'a>(
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
fn empty_remove(doc: &mut DocumentMut, name: &str) {
    let empty = doc
        .get(name)
        .and_then(Item::as_table_like)
        .is_some_and(|t| t.is_empty());
    if empty {
        doc.remove(name);
    }
}

fn list(entries: &[String]) -> Item {
    value(entries.iter().map(String::as_str).collect::<Array>())
}

/// `(url, model)` of an endpoint table; the model may stand under the older name `modell`.
fn endpoint_values(item: Option<&Item>) -> Option<(Option<String>, Option<String>)> {
    let t = item?.as_table_like()?;
    let text = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| t.get(k).and_then(Item::as_str))
            .map(str::to_string)
    };
    Some((text(&["url"]), text(&["model", "modell"])))
}

/// Writes provider and endpoint in the current format (`[agent] provider`, `[agent.endpoint]`).
/// The older spellings are adopted and removed so that only one place applies: `[litellm]`,
/// `[agent] anbieter` and `[agent.endpunkt]` (with `modell`).
fn endpoint_write(
    doc: &mut DocumentMut,
    e: &cockpit_core::api::EndpointInput,
) -> Result<(), String> {
    let url = e.url.as_deref().map(endpoint_url_read).transpose()?;
    let model = match e.model.as_deref().map(str::trim) {
        Some(m) if m.chars().any(|c| c.is_whitespace() || c.is_control()) => {
            return Err("Der Modellname enthält Leerzeichen oder Sonderzeichen.".into());
        }
        other => other,
    };
    // Rescue values from an older `[litellm]` before it disappears.
    let legacy_litellm = endpoint_values(doc.get("litellm"));
    if legacy_litellm.is_some() {
        doc.remove("litellm");
    }
    {
        let agent = table(doc, "agent")?;
        if let Some(old) = agent.remove("anbieter")
            && !agent.contains_key("provider")
        {
            agent.insert("provider", old);
        }
        if let Some(provider) = e.provider {
            set(agent, "provider", value(provider.id()));
        }
        let legacy_endpoint = agent.remove("endpunkt");
        let legacy = endpoint_values(legacy_endpoint.as_ref());
        if !agent.contains_key("endpoint") {
            agent.insert("endpoint", Item::Table(Table::new()));
        }
        let ep = agent
            .get_mut("endpoint")
            .and_then(Item::as_table_like_mut)
            .ok_or("In config.toml ist „agent.endpoint“ keine Tabelle – bitte die Datei prüfen.")?;
        // Several spellings at once: the current one wins, then `[agent.endpunkt]`, then `[litellm]`.
        for (old_url, old_model) in [legacy, legacy_litellm].into_iter().flatten() {
            if !ep.contains_key("url")
                && let Some(u) = old_url
            {
                ep.insert("url", value(u));
            }
            if !ep.contains_key("model")
                && let Some(m) = old_model
            {
                ep.insert("model", value(m));
            }
        }
        ep.remove("modell");
        match url.as_deref() {
            Some("") => {
                ep.remove("url");
            }
            Some(u) => set(ep, "url", value(u)),
            None => {}
        }
        match model {
            Some("") => {
                ep.remove("model");
            }
            Some(m) => set(ep, "model", value(m)),
            None => {}
        }
        if ep.is_empty() {
            agent.remove("endpoint");
        }
    }
    empty_remove(doc, "agent");
    Ok(())
}

/// Wendet die Änderung auf den Text von `config.toml` an. Der Text wird anschließend wie beim
/// Start geprüft (`Config::parse`); ein Fehler heißt: nichts schreiben.
pub fn change(text: &str, a: &ConfigChange) -> Result<(String, Config), String> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        format!("config.toml ist fehlerhaft: {}", e.message())
    })?;

    if let Some(j) = &a.jira {
        {
            let t = table(&mut doc, "jira")?;
            if let Some(site) = &j.site {
                let site = jira_site_read(site)?;
                if site.is_empty() {
                    t.remove("site");
                } else {
                    set(t, "site", value(site));
                }
            }
            if let Some(email) = &j.email {
                let email = email.trim();
                if email.is_empty() {
                    t.remove("email");
                } else if !email.contains('@') || email.contains(char::is_whitespace) {
                    return Err("Das ist keine E-Mail-Adresse.".into());
                } else {
                    set(t, "email", value(email));
                }
            }
            if let Some(p) = &j.projects {
                let projects = projects_read(&p.join(","));
                if projects.is_empty() {
                    t.remove("projects");
                } else {
                    set(t, "projects", list(&projects));
                }
            }
        }
        empty_remove(&mut doc, "jira");
    }
    for (name, input) in [
        ("atlassian", &a.atlassian),
        ("github_oauth", &a.github_oauth),
    ] {
        if let Some(c) = input {
            let id = client_id_read(&c.client_id)?;
            if id.is_empty() {
                doc.remove(name);
            } else {
                set(table(&mut doc, name)?, "client_id", value(id));
            }
        }
    }
    if let Some(g) = &a.github {
        let repos = repos_read(&g.repos.join(","))?;
        {
            let t = table(&mut doc, "github")?;
            if repos.is_empty() {
                t.remove("repos");
            } else {
                set(t, "repos", list(&repos));
            }
        }
        empty_remove(&mut doc, "github");
    }
    if let Some(e) = &a.endpoint {
        endpoint_write(&mut doc, e)?;
    }

    let updated = doc.to_string();
    let config = Config::parse(Some(&updated), |_| None).map_err(|e| match e {
        ConfigError::Wert(m) => m,
        other => other.to_string(),
    })?;
    Ok((updated, config))
}

/// Schreibt die Datei atomar: erst eine Nachbardatei, dann umbenennen. Eine vorhandene Datei
/// behält ihre Rechte; eine neue ist nur für den Nutzer lesbar.
pub fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    // Ein Link auf die Datei bleibt ein Link: Wir ersetzen das Ziel.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".into());
    let tmp = dir.join(format!(".{name}.neu-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        if let Ok(alt) = std::fs::metadata(&path) {
            std::fs::set_permissions(&tmp, alt.permissions())?;
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

impl AppState {
    /// Übernimmt die Adressen und Konten aus der Konfiguration in die Datenbank (ohne Schlüssel).
    /// Blockierend.
    pub(crate) fn connections_adopt(&self, config: &Config) -> Result<(), String> {
        let conn = self
            .inner
            .db
            .lock()
            .map_err(|_| "Datenbank gesperrt".to_string())?;
        if let Some(g) = &config.github {
            db::connection_set(&conn, secrets::GITHUB, &g.api_url, None)
                .map_err(|e| e.to_string())?;
        }
        // Mit OAuth hat Jira keine eigene Adresse; die Site wählt die Anmeldung.
        if let Some(j) = config.jira.as_ref().filter(|j| !j.site.is_empty()) {
            db::connection_set(&conn, secrets::JIRA, &j.site, Some(&j.email))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Ändert `config.toml` und wendet das Ergebnis an. Blockierend.
    fn config_change(&self, a: &ConfigChange) -> Result<ConfigResponse, (StatusCode, String)> {
        let Some(path) = self.inner.settings.config_path.clone() else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Dieser Rechner hat keinen Ort für die Konfigurationsdatei. Setze COCKPIT_CONFIG auf einen Dateipfad."
                    .into(),
            ));
        };
        let _lock = self.inner.config_lock.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Interner Fehler".to_string(),
            )
        })?;
        let alt = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                tracing::warn!("config.toml nicht lesbar: {e}");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Die Datei {} lässt sich nicht lesen.", path.display()),
                ));
            }
        };
        let (updated, config) =
            change(&alt, a).map_err(|m| (StatusCode::UNPROCESSABLE_ENTITY, m))?;
        let before = self.sources();
        if updated != alt {
            atomic_write(&path, &updated).map_err(|e| {
                tracing::warn!("config.toml nicht geschrieben: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Die Datei {} lässt sich nicht schreiben ({}).",
                        path.display(),
                        e.kind()
                    ),
                )
            })?;
        }
        self.connections_adopt(&config)
            .map_err(|m| (StatusCode::INTERNAL_SERVER_ERROR, m))?;
        self.config_apply(&config);
        tracing::info!("config.toml aktualisiert und angewendet");

        // Eine andere App heißt: Die alten Tokens gehören zur alten App.
        let other_app = |alt: Option<&String>, updated: Option<&String>| {
            alt.is_some() && updated.is_some() && alt != updated
        };
        let id_alt_g = before.github_oauth.as_ref().map(|o| &o.client_id);
        let id_new_g = config.github_oauth.as_ref().map(|o| &o.client_id);
        let id_alt_a = before.atlassian.as_ref().map(|o| &o.client_id);
        let id_new_a = config.atlassian.as_ref().map(|o| &o.client_id);
        let hint = (other_app(id_alt_g, id_new_g) || other_app(id_alt_a, id_new_a)).then(|| {
            "Du hast die Client-ID einer App geändert. Melde dich danach neu an, damit die Anmeldung zur neuen App gehört."
                .to_string()
        });
        Ok(ConfigResponse {
            stored: true,
            takes_effect_immediately: true,
            hint,
        })
    }

    /// Die aktuellen, nicht geheimen Werte. Blockierend (Schlüsselbund).
    fn setup(&self) -> Setup {
        let q = self.sources();
        let choice = self.agent_choice();
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
            provider: choice.provider,
            endpoint_url: choice.endpoint.as_ref().and_then(|e| e.url.clone()),
            endpoint_model: choice.endpoint.and_then(|e| e.model),
            callback_github: redirect_uri(Service::GitHub, port),
            callback_atlassian: redirect_uri(Service::Atlassian, port),
            keys: KEY_NAMES
                .iter()
                .map(|n| KeyResponse {
                    name: (*n).to_string(),
                    set: self.inner.secrets.has(n).unwrap_or(false),
                })
                .collect(),
        }
    }
}

/// `GET /api/setup`.
pub async fn read(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.setup()).await {
        Ok(e) => Json(e).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `PUT /api/config`.
pub async fn config(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(a) = serde_json::from_slice::<ConfigChange>(&body) else {
        return error(StatusCode::BAD_REQUEST, "Die Eingaben sind nicht lesbar");
    };
    match tokio::task::spawn_blocking(move || state.config_change(&a)).await {
        Ok(Ok(answer)) => Json(answer).into_response(),
        Ok(Err((status, message))) => error(status, message),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `PUT /api/secrets/{name}`: legt den Schlüssel im Schlüsselbund ab. Der Wert steht nur im
/// Body; er wird weder geloggt noch zurückgeschickt, und Fehlermeldungen nennen ihn nie.
pub async fn key(
    State(state): State<AppState>,
    UrlPath(name): UrlPath<String>,
    body: Bytes,
) -> Response {
    if !key_name_allowed(&name) {
        return error(
            StatusCode::NOT_FOUND,
            format!("Einen Schlüssel „{name}“ gibt es nicht"),
        );
    }
    let Ok(input) = serde_json::from_slice::<KeyInput>(&body) else {
        return error(StatusCode::BAD_REQUEST, "Die Eingabe ist nicht lesbar");
    };
    let wert = match key_verify(&input.value) {
        Ok(w) => w,
        Err(m) => return error(StatusCode::UNPROCESSABLE_ENTITY, m),
    };
    let secrets = state.inner.secrets.clone();
    let n = name.clone();
    let result = tokio::task::spawn_blocking(move || secrets.set(&n, &Secret::new(wert))).await;
    match result {
        Ok(Ok(())) => {
            tracing::info!("Schlüssel für {name} abgelegt");
            Json(KeyResponse { name, set: true }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Schlüssel für {name} nicht abgelegt: {e}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Schlüsselbund hat den Schlüssel nicht angenommen. Prüfe, ob der Schlüsselbund entsperrt ist.",
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `DELETE /api/secrets/{name}`: löscht einen Token aus dem Schlüsselbund („Trennen“). Erlaubt
/// sind nur die Tokens `jira` und `github`; ein nicht vorhandener Schlüssel ist kein Fehler.
pub async fn key_delete(State(state): State<AppState>, UrlPath(name): UrlPath<String>) -> Response {
    if !key_deletable(&name) {
        return error(
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
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Der Schlüsselbund hat den Schlüssel nicht gelöscht. Prüfe, ob der Schlüsselbund entsperrt ist.",
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn check(cause: JiraCause, name: Option<String>) -> JiraCheck {
    JiraCheck {
        ok: cause == JiraCause::Ok,
        cause,
        message: jira_check_text(cause, name.as_deref()),
        name: name.filter(|n| !n.is_empty()),
    }
}

/// Welche Ursache zu einem Fehler von Jira gehört.
fn jira_cause(f: &Error) -> JiraCause {
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
    pub async fn jira_check(&self) -> JiraCheck {
        let s = self.clone();
        let details = tokio::task::spawn_blocking(move || {
            let j = s.sources().jira.clone().filter(|j| !j.site.is_empty());
            j.map(|j| (j.site, j.email, s.token(crate::secrets::JIRA)))
        })
        .await
        .ok()
        .flatten();
        let Some((site, email, Some(token))) = details.filter(|(_, e, _)| !e.is_empty()) else {
            return check(JiraCause::Details, None);
        };
        let client = match JiraClient::new(&site, &email, token) {
            Ok(c) => c,
            Err(e) => return check(jira_cause(&e), None),
        };
        match client.my_name().await {
            Ok(name) => check(JiraCause::Ok, Some(name)),
            Err(e) => {
                tracing::warn!("Jira-Verbindungstest: {e}");
                check(jira_cause(&e), None)
            }
        }
    }
}

/// `POST /api/jira/check`: ohne Body; liest nur.
pub async fn jira_check(State(state): State<AppState>) -> Response {
    Json(state.jira_check().await).into_response()
}

#[cfg(test)]
mod tests;
