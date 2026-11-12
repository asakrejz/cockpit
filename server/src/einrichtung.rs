//! Einrichtung per Klick: Routen für den Assistenten in der App.
//!
//! * `GET /api/einrichtung`: aktuelle, nicht geheime Werte und welche Schlüssel liegen
//! * `PUT /api/konfiguration`: schreibt Client-IDs, Jira, Repos und LiteLLM in `config.toml`
//!   (atomar, Kommentare und andere Werte bleiben) und wendet sie ohne Neustart an
//! * `PUT /api/schluessel/{name}`: legt einen Schlüssel im Schlüsselbund ab
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
    Einrichtung, KonfigAenderung, KonfigAntwort, SchluesselAntwort, SchluesselEingabe,
};
use cockpit_core::einrichten::{
    SCHLUESSEL_NAMEN, client_id_lesen, jira_site_lesen, litellm_url_lesen, projekte_lesen,
    repos_lesen, schluessel_name_erlaubt, schluessel_pruefen,
};
use cockpit_integrations::Secret;
use cockpit_integrations::oauth::Dienst;
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

/// Wendet die Änderung auf den Text von `config.toml` an. Der Text wird anschließend wie beim
/// Start geprüft (`Config::aus`); ein Fehler heißt: nichts schreiben.
pub fn aendern(text: &str, a: &KonfigAenderung) -> Result<(String, Config), String> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        format!("config.toml ist fehlerhaft: {}", e.message())
    })?;

    if let Some(j) = &a.jira {
        {
            let t = tabelle(&mut doc, "jira")?;
            if let Some(site) = &j.site {
                let site = jira_site_lesen(site)?;
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
                let projekte = projekte_lesen(&p.join(","));
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
            let id = client_id_lesen(&c.client_id)?;
            if id.is_empty() {
                doc.remove(name);
            } else {
                setzen(tabelle(&mut doc, name)?, "client_id", value(id));
            }
        }
    }
    if let Some(g) = &a.github {
        let repos = repos_lesen(&g.repos.join(","))?;
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
    if let Some(l) = &a.litellm {
        let url = litellm_url_lesen(&l.url)?;
        if url.is_empty() {
            doc.remove("litellm");
        } else {
            let t = tabelle(&mut doc, "litellm")?;
            setzen(t, "url", value(url));
            match l.modell.as_deref().map(str::trim) {
                Some("") => {
                    t.remove("modell");
                }
                Some(m) => setzen(t, "modell", value(m)),
                None => {}
            }
        }
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
        a: &KonfigAenderung,
    ) -> Result<KonfigAntwort, (StatusCode, String)> {
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
        Ok(KonfigAntwort {
            gespeichert: true,
            wirkt_sofort: true,
            hinweis,
        })
    }

    /// Die aktuellen, nicht geheimen Werte. Blockierend (Schlüsselbund).
    fn einrichtung(&self) -> Einrichtung {
        let q = self.quellen();
        let proxy = self.litellm();
        let port = self.inner.port;
        Einrichtung {
            jira_site: q.jira.as_ref().map(|j| j.site.clone()).unwrap_or_default(),
            jira_email: q.jira.as_ref().map(|j| j.email.clone()).unwrap_or_default(),
            jira_projekte: q
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
            litellm_url: proxy.as_ref().map(|l| l.url.clone()),
            litellm_modell: proxy.and_then(|l| l.modell),
            callback_github: redirect_uri(Dienst::GitHub, port),
            callback_atlassian: redirect_uri(Dienst::Atlassian, port),
            schluessel: SCHLUESSEL_NAMEN
                .iter()
                .map(|n| SchluesselAntwort {
                    name: (*n).to_string(),
                    gesetzt: self.inner.secrets.has(n).unwrap_or(false),
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
    let Ok(a) = serde_json::from_slice::<KonfigAenderung>(&body) else {
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
    if !schluessel_name_erlaubt(&name) {
        return fehler(
            StatusCode::NOT_FOUND,
            format!("Einen Schlüssel „{name}“ gibt es nicht"),
        );
    }
    let Ok(eingabe) = serde_json::from_slice::<SchluesselEingabe>(&body) else {
        return fehler(StatusCode::BAD_REQUEST, "Die Eingabe ist nicht lesbar");
    };
    let wert = match schluessel_pruefen(&eingabe.wert) {
        Ok(w) => w,
        Err(m) => return fehler(StatusCode::UNPROCESSABLE_ENTITY, m),
    };
    let secrets = state.inner.secrets.clone();
    let n = name.clone();
    let ergebnis = tokio::task::spawn_blocking(move || secrets.set(&n, &Secret::new(wert))).await;
    match ergebnis {
        Ok(Ok(())) => {
            tracing::info!("Schlüssel für {name} abgelegt");
            Json(SchluesselAntwort {
                name,
                gesetzt: true,
            })
            .into_response()
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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::Request;
    use cockpit_core::api::{ClientEingabe, GitHubEingabe, JiraEingabe, LitellmEingabe};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::abgleich::Quellen;
    use crate::app::{Einstellungen, router};
    use crate::secrets::{MemoryStore, SecretStore};

    const MIT_KOMMENTAREN: &str = "# Meine Einstellungen\nport = 8787 # Standard\n\n[github]\n# Diese Repos gleicht Cockpit ab\nrepos = [\"alt/repo\"] # alt\n";

    fn aenderung_github(repos: &[&str]) -> KonfigAenderung {
        KonfigAenderung {
            github: Some(GitHubEingabe {
                repos: repos.iter().map(|r| (*r).to_string()).collect(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn aendern_behaelt_kommentare_und_andere_werte() {
        let (neu, c) = aendern(
            MIT_KOMMENTAREN,
            &aenderung_github(&["firma/app", "firma/web"]),
        )
        .unwrap();
        assert!(neu.contains("# Meine Einstellungen"), "{neu}");
        assert!(neu.contains("port = 8787 # Standard"), "{neu}");
        assert!(neu.contains("# Diese Repos gleicht Cockpit ab"), "{neu}");
        assert!(neu.contains("# alt"), "{neu}");
        assert_eq!(c.github.unwrap().repos, vec!["firma/app", "firma/web"]);
    }

    #[test]
    fn aendern_legt_abschnitte_in_einer_leeren_datei_an() {
        let a = KonfigAenderung {
            atlassian: Some(ClientEingabe {
                client_id: " 7dNimh ".into(),
            }),
            github_oauth: Some(ClientEingabe {
                client_id: "Iv23li".into(),
            }),
            jira: Some(JiraEingabe {
                projects: Some(vec!["bau".into(), "web".into()]),
                ..Default::default()
            }),
            litellm: Some(LitellmEingabe {
                url: "localhost:4000/".into(),
                modell: Some("claude-sonnet-5-5".into()),
            }),
            ..Default::default()
        };
        let (neu, c) = aendern("", &a).unwrap();
        assert_eq!(c.atlassian.unwrap().client_id, "7dNimh");
        assert_eq!(c.github_oauth.unwrap().client_id, "Iv23li");
        assert_eq!(c.jira.unwrap().projects, vec!["BAU", "WEB"]);
        let l = c.litellm.unwrap();
        assert_eq!(l.url, "http://localhost:4000");
        assert_eq!(l.modell.as_deref(), Some("claude-sonnet-5-5"));
        // Ergebnis ist wieder lesbar wie beim Start.
        assert!(Config::aus(Some(&neu), |_| None).is_ok());
    }

    #[test]
    fn aendern_entfernt_mit_leeren_werten() {
        let start = "[atlassian]\nclient_id = \"x\"\n[litellm]\nurl = \"http://a:1\"\nmodell = \"m\"\n[github]\nrepos = [\"a/b\"]\n";
        let a = KonfigAenderung {
            atlassian: Some(ClientEingabe::default()),
            litellm: Some(LitellmEingabe::default()),
            github: Some(GitHubEingabe::default()),
            ..Default::default()
        };
        let (neu, c) = aendern(start, &a).unwrap();
        assert!(c.atlassian.is_none() && c.litellm.is_none());
        assert!(c.github.is_none(), "{neu}");
    }

    #[test]
    fn aendern_prueft_wie_der_start() {
        // Jira ohne OAuth braucht Site und E-Mail.
        let nur_projekte = KonfigAenderung {
            jira: Some(JiraEingabe {
                projects: Some(vec!["BAU".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(aendern("", &nur_projekte).is_err());
        let token = KonfigAenderung {
            jira: Some(JiraEingabe {
                site: Some("firma".into()),
                email: Some("ich@firma.example".into()),
                projects: None,
            }),
            ..Default::default()
        };
        let (_, c) = aendern("", &token).unwrap();
        let j = c.jira.unwrap();
        assert_eq!(j.site, "https://firma.atlassian.net");
        assert_eq!(j.email, "ich@firma.example");

        for schlecht in [
            aenderung_github(&["kein-repo"]),
            KonfigAenderung {
                jira: Some(JiraEingabe {
                    email: Some("keine-adresse".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            KonfigAenderung {
                litellm: Some(LitellmEingabe {
                    url: "ftp://x".into(),
                    modell: None,
                }),
                ..Default::default()
            },
            KonfigAenderung {
                github_oauth: Some(ClientEingabe {
                    client_id: "mit leerzeichen".into(),
                }),
                ..Default::default()
            },
        ] {
            assert!(aendern("", &schlecht).is_err(), "{schlecht:?}");
        }
        // Eine kaputte Datei wird nicht überschrieben.
        assert!(aendern("port = [", &aenderung_github(&["a/b"])).is_err());
    }

    #[test]
    fn schreiben_ist_atomar_und_hinterlaesst_nichts() {
        let ordner = tempfile::tempdir().unwrap();
        let pfad = ordner.path().join("unter").join("config.toml");
        atomar_schreiben(&pfad, "a = 1\n").unwrap();
        atomar_schreiben(&pfad, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&pfad).unwrap(), "a = 2\n");
        let uebrig: Vec<_> = std::fs::read_dir(pfad.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(uebrig.len(), 1, "{uebrig:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&pfad).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    struct Prufstand {
        app: Router,
        secrets: Arc<MemoryStore>,
        pfad: PathBuf,
        _ordner: tempfile::TempDir,
    }

    fn pruefstand(text: Option<&str>) -> Prufstand {
        let ordner = tempfile::tempdir().unwrap();
        let pfad = ordner.path().join("config.toml");
        if let Some(t) = text {
            std::fs::write(&pfad, t).unwrap();
        }
        let secrets = Arc::new(MemoryStore::new());
        let e = Einstellungen {
            suchpfad: Some(std::ffi::OsString::new()),
            konfig_pfad: Some(pfad.clone()),
            ..Einstellungen::default()
        };
        let state = AppState::neu(
            db::open_im_speicher().unwrap(),
            secrets.clone(),
            8787,
            Quellen::default(),
            e,
        );
        Prufstand {
            app: router(state, PathBuf::from("/x")),
            secrets,
            pfad,
            _ordner: ordner,
        }
    }

    fn anfrage(methode: &str, pfad: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(methode)
            .uri(pfad)
            .header("host", "127.0.0.1:8787")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
    }

    async fn sende(app: &Router, req: Request<Body>) -> (StatusCode, String) {
        let antwort = app.clone().oneshot(req).await.unwrap();
        let status = antwort.status();
        let body = antwort.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn put(app: &Router, pfad: &str, json: &str) -> (StatusCode, String) {
        sende(
            app,
            anfrage("PUT", pfad)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
        )
        .await
    }

    async fn get(app: &Router, pfad: &str) -> serde_json::Value {
        let (status, body) = sende(app, anfrage("GET", pfad).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        serde_json::from_str(&body).unwrap()
    }

    #[tokio::test]
    async fn schluessel_landet_im_schluesselbund_und_kommt_nie_zurueck() {
        let p = pruefstand(None);
        let (status, body) = put(
            &p.app,
            "/api/schluessel/github-oauth",
            r#"{"wert":"  test-wert-123 "}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(!body.contains("test-wert-123"), "{body}");
        assert!(body.contains(r#""gesetzt":true"#));
        assert_eq!(
            p.secrets.get("github-oauth").unwrap().unwrap().expose(),
            "test-wert-123"
        );
        // Weder die Einrichtung noch die Gesundheitsmeldung zeigen den Wert.
        let e = get(&p.app, "/api/einrichtung").await;
        assert!(!e.to_string().contains("test-wert-123"));
        let flag = |n: &str| {
            e["schluessel"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["name"] == n)
                .unwrap()["gesetzt"]
                .as_bool()
                .unwrap()
        };
        assert!(flag("github-oauth") && !flag("atlassian-oauth"));
        let h = get(&p.app, "/api/health").await;
        assert!(!h.to_string().contains("test-wert-123"));
    }

    #[tokio::test]
    async fn schluessel_nur_mit_erlaubtem_namen_und_gueltigem_wert() {
        let p = pruefstand(None);
        for name in ["github-oauth-token", "unsinn", "Jira"] {
            let (status, body) = put(
                &p.app,
                &format!("/api/schluessel/{name}"),
                r#"{"wert":"test-wert-123"}"#,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{name}");
            assert!(!body.contains("test-wert-123"));
        }
        assert!(p.secrets.get("github-oauth-token").unwrap().is_none());
        for (body, erwartet) in [
            (r#"{"wert":""}"#, StatusCode::UNPROCESSABLE_ENTITY),
            (r#"{"wert":"  "}"#, StatusCode::UNPROCESSABLE_ENTITY),
            (
                r#"{"wert":"test-wert\n123"}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            ("kein json test-wert-123", StatusCode::BAD_REQUEST),
            (r#"{"anders":"test-wert-123"}"#, StatusCode::BAD_REQUEST),
        ] {
            let (status, antwort) = put(&p.app, "/api/schluessel/litellm", body).await;
            assert_eq!(status, erwartet, "{body}");
            assert!(!antwort.contains("test-wert"), "{antwort}");
        }
        assert!(p.secrets.get("litellm").unwrap().is_none());
    }

    #[tokio::test]
    async fn neue_routen_haben_dieselben_pruefungen_wie_alle_api_routen() {
        let p = pruefstand(None);
        let wert = r#"{"wert":"test-wert-123"}"#;
        // Content-Type muss JSON sein.
        for typ in [
            None,
            Some("text/plain"),
            Some("application/x-www-form-urlencoded"),
        ] {
            let mut b = anfrage("PUT", "/api/schluessel/litellm");
            if let Some(t) = typ {
                b = b.header("content-type", t);
            }
            let (status, _) = sende(&p.app, b.body(Body::from(wert)).unwrap()).await;
            assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        // Fremde Herkunft, fremder Host, fremde Gegenstelle.
        let (status, _) = sende(
            &p.app,
            anfrage("PUT", "/api/schluessel/litellm")
                .header("content-type", "application/json")
                .header("origin", "https://boese.example")
                .body(Body::from(wert))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = sende(
            &p.app,
            Request::builder()
                .method("PUT")
                .uri("/api/schluessel/litellm")
                .header("host", "boese.example")
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
                .body(Body::from(wert))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = sende(
            &p.app,
            Request::builder()
                .method("PUT")
                .uri("/api/konfiguration")
                .header("host", "127.0.0.1:8787")
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = sende(
            &p.app,
            Request::builder()
                .uri("/api/einrichtung")
                .header("host", "boese.example")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(p.secrets.get("litellm").unwrap().is_none());
        assert!(!p.pfad.exists());
    }

    #[tokio::test]
    async fn konfiguration_schreibt_die_datei_und_wirkt_ohne_neustart() {
        let p = pruefstand(Some(MIT_KOMMENTAREN));
        // Vorher: nichts eingerichtet.
        let h = get(&p.app, "/api/health").await;
        assert_eq!(h["litellm"]["konfiguriert"], false);

        let body = r#"{"litellm":{"url":"localhost:4000","modell":"claude-sonnet-5-5"},
            "github_oauth":{"client_id":"Iv23li"},
            "github":{"repos":["firma/app","https://github.com/firma/web.git"]}}"#;
        let (status, antwort) = put(&p.app, "/api/konfiguration", body).await;
        assert_eq!(status, StatusCode::OK, "{antwort}");
        let a: KonfigAntwort = serde_json::from_str(&antwort).unwrap();
        assert!(a.gespeichert && a.wirkt_sofort);

        // Datei: neu, aber Kommentare und alte Werte bleiben.
        let datei = std::fs::read_to_string(&p.pfad).unwrap();
        assert!(
            datei.contains("# Meine Einstellungen") && datei.contains("port = 8787 # Standard")
        );
        assert!(
            datei.contains("firma/web") && !datei.contains("alt/repo"),
            "{datei}"
        );
        assert!(Config::aus(Some(&datei), |_| None).is_ok());

        // Ohne Neustart sichtbar.
        let h = get(&p.app, "/api/health").await;
        assert_eq!(h["litellm"]["url"], "http://localhost:4000");
        assert_eq!(h["litellm"]["modell"], "claude-sonnet-5-5");
        let e = get(&p.app, "/api/einrichtung").await;
        assert_eq!(e["github_client_id"], "Iv23li");
        assert_eq!(
            e["github_repos"],
            serde_json::json!(["firma/app", "firma/web"])
        );
        assert_eq!(
            e["callback_github"],
            "http://127.0.0.1:8787/api/oauth/github/callback"
        );
        assert_eq!(
            e["callback_atlassian"],
            "http://localhost:8787/api/oauth/atlassian/callback"
        );
        // Mit Client-ID und Secret ist „Mit GitHub anmelden“ möglich.
        put(
            &p.app,
            "/api/schluessel/github-oauth",
            r#"{"wert":"test-wert-123"}"#,
        )
        .await;
        let h = get(&p.app, "/api/health").await;
        assert_eq!(h["anmeldung"]["github"]["oauth_moeglich"], true);
    }

    #[tokio::test]
    async fn ungueltige_eingaben_aendern_nichts_und_sagen_warum() {
        let p = pruefstand(Some(MIT_KOMMENTAREN));
        let (status, body) = put(
            &p.app,
            "/api/konfiguration",
            r#"{"github":{"repos":["kein-repo"]}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body.contains("kein-repo") && body.contains("firma/app"),
            "{body}"
        );
        assert_eq!(std::fs::read_to_string(&p.pfad).unwrap(), MIT_KOMMENTAREN);
        let (status, _) = put(&p.app, "/api/konfiguration", "kein json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn neue_client_id_bringt_den_hinweis_zur_neuen_anmeldung() {
        let p = pruefstand(None);
        let id = |i: &str| format!(r#"{{"atlassian":{{"client_id":"{i}"}}}}"#);
        let (_, erste) = put(&p.app, "/api/konfiguration", &id("alt")).await;
        assert!(
            serde_json::from_str::<KonfigAntwort>(&erste)
                .unwrap()
                .hinweis
                .is_none()
        );
        let (_, zweite) = put(&p.app, "/api/konfiguration", &id("neu")).await;
        assert!(
            serde_json::from_str::<KonfigAntwort>(&zweite)
                .unwrap()
                .hinweis
                .is_some()
        );
    }

    #[tokio::test]
    async fn ohne_datei_wird_sie_angelegt() {
        let p = pruefstand(None);
        let (status, _) = put(
            &p.app,
            "/api/konfiguration",
            r#"{"jira":{"site":"firma","email":"ich@firma.example"}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            std::fs::read_to_string(&p.pfad)
                .unwrap()
                .contains("https://firma.atlassian.net")
        );
        // Jira per Token braucht noch den Schlüssel: erst danach „verbunden“.
        let h = get(&p.app, "/api/health").await;
        assert_eq!(h["anmeldung"]["jira"]["verbunden"], false);
        put(
            &p.app,
            "/api/schluessel/jira",
            r#"{"wert":"test-wert-123"}"#,
        )
        .await;
        let h = get(&p.app, "/api/health").await;
        assert_eq!(h["anmeldung"]["jira"]["verbunden"], true);
    }
}
