use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use cockpit_core::api::{ClientInput, EndpointInput, GitHubInput, JiraInput, Provider};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::abgleich::Quellen;
use crate::app::{Einstellungen, router};
use crate::secrets::{MemoryStore, SecretStore};

const MIT_KOMMENTAREN: &str = "# Meine Einstellungen\nport = 8787 # Standard\n\n[github]\n# Diese Repos gleicht Cockpit ab\nrepos = [\"alt/repo\"] # alt\n";

fn aenderung_github(repos: &[&str]) -> ConfigChange {
    ConfigChange {
        github: Some(GitHubInput {
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
    let a = ConfigChange {
        atlassian: Some(ClientInput {
            client_id: " 7dNimh ".into(),
        }),
        github_oauth: Some(ClientInput {
            client_id: "Iv23li".into(),
        }),
        jira: Some(JiraInput {
            projects: Some(vec!["bau".into(), "web".into()]),
            ..Default::default()
        }),
        endpoint: Some(EndpointInput {
            provider: Some(Provider::ClaudeAgentSdk),
            url: Some("localhost:4000/".into()),
            model: Some("claude-sonnet-5-5".into()),
        }),
        ..Default::default()
    };
    let (neu, c) = aendern("", &a).unwrap();
    assert_eq!(c.atlassian.unwrap().client_id, "7dNimh");
    assert_eq!(c.github_oauth.unwrap().client_id, "Iv23li");
    assert_eq!(c.jira.unwrap().projects, vec!["BAU", "WEB"]);
    let l = c.endpunkt.unwrap();
    assert_eq!(l.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(l.modell.as_deref(), Some("claude-sonnet-5-5"));
    // Das neue Format, kein `[litellm]`.
    assert!(
        neu.contains("[agent.endpunkt]") && !neu.contains("litellm"),
        "{neu}"
    );
    assert!(neu.contains("anbieter = \"claude-agent-sdk\""), "{neu}");
    // Ergebnis ist wieder lesbar wie beim Start.
    assert!(Config::aus(Some(&neu), |_| None).is_ok());
}

#[test]
fn aendern_uebernimmt_ein_aelteres_litellm_und_behaelt_agent_werte() {
    let alt = "[litellm]\nurl = \"http://a:1\"\nmodell = \"altes-modell\"\n[agent]\ncontainer = \"podman\" # meins\nrepo = \"firma/app\"\n";
    // Nur das Modell ändern: die alte Adresse bleibt, `[litellm]` wandert.
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            model: Some("neues-modell".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (neu, c) = aendern(alt, &a).unwrap();
    assert!(!neu.contains("[litellm]"), "{neu}");
    assert!(neu.contains("container = \"podman\" # meins"), "{neu}");
    let e = c.endpunkt.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://a:1"));
    assert_eq!(e.modell.as_deref(), Some("neues-modell"));
    assert_eq!(c.container.as_deref(), Some("podman"));
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
}

#[test]
fn aendern_wechselt_auf_direkt_ohne_adresse() {
    let start = "[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n";
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            provider: Some(Provider::ClaudeAgentSdk),
            url: Some(String::new()),
            model: Some("claude-opus-5-5".into()),
        }),
        ..Default::default()
    };
    let (neu, c) = aendern(start, &a).unwrap();
    assert!(!neu.contains("url"), "{neu}");
    let e = c.endpunkt.unwrap();
    assert_eq!(
        (e.url, e.modell.as_deref()),
        (None, Some("claude-opus-5-5"))
    );
    // Ein noch nicht umgesetztes Werkzeug lässt sich eintragen.
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            provider: Some(Provider::Codex),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(aendern("", &a).unwrap().1.anbieter, Provider::Codex);
}

#[test]
fn aendern_entfernt_mit_leeren_werten() {
    let start = "[atlassian]\nclient_id = \"x\"\n[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n[github]\nrepos = [\"a/b\"]\n";
    let a = ConfigChange {
        atlassian: Some(ClientInput::default()),
        endpoint: Some(EndpointInput {
            url: Some(String::new()),
            model: Some(String::new()),
            ..Default::default()
        }),
        github: Some(GitHubInput::default()),
        ..Default::default()
    };
    let (neu, c) = aendern(start, &a).unwrap();
    assert!(c.atlassian.is_none() && c.endpunkt.is_none());
    assert!(!neu.contains("agent"), "{neu}");
    assert!(c.github.is_none(), "{neu}");
}

#[test]
fn aendern_prueft_wie_der_start() {
    // Jira ohne OAuth braucht Site und E-Mail.
    let nur_projekte = ConfigChange {
        jira: Some(JiraInput {
            projects: Some(vec!["BAU".into()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(aendern("", &nur_projekte).is_err());
    let token = ConfigChange {
        jira: Some(JiraInput {
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
        ConfigChange {
            jira: Some(JiraInput {
                email: Some("keine-adresse".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ConfigChange {
            endpoint: Some(EndpointInput {
                url: Some("ftp://x".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ConfigChange {
            github_oauth: Some(ClientInput {
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
async fn der_agent_key_geht_in_den_schluesselbund_und_nie_zurueck() {
    let p = pruefstand(None);
    let (status, antwort) = put(
        &p.app,
        "/api/schluessel/agent-key",
        r#"{"wert":"sk-test-123"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{antwort}");
    assert!(!antwort.contains("sk-test-123"), "{antwort}");
    assert!(p.secrets.get("agent-key").unwrap().is_some());
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpunkt"]["key"], true);
    assert!(!h.to_string().contains("sk-test-123"));
    let e = get(&p.app, "/api/einrichtung").await;
    assert!(!e.to_string().contains("sk-test-123"));
}

#[tokio::test]
async fn konfiguration_schreibt_die_datei_und_wirkt_ohne_neustart() {
    let p = pruefstand(Some(MIT_KOMMENTAREN));
    // Vorher: nichts eingerichtet.
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpunkt"]["konfiguriert"], false);

    let body = r#"{"endpunkt":{"anbieter":"claude-agent-sdk","url":"localhost:4000","modell":"claude-sonnet-5-5"},
        "github_oauth":{"client_id":"Iv23li"},
        "github":{"repos":["firma/app","https://github.com/firma/web.git"]}}"#;
    let (status, antwort) = put(&p.app, "/api/konfiguration", body).await;
    assert_eq!(status, StatusCode::OK, "{antwort}");
    let a: ConfigResponse = serde_json::from_str(&antwort).unwrap();
    assert!(a.stored && a.takes_effect_immediately);

    // Datei: neu, aber Kommentare und alte Werte bleiben.
    let datei = std::fs::read_to_string(&p.pfad).unwrap();
    assert!(datei.contains("# Meine Einstellungen") && datei.contains("port = 8787 # Standard"));
    assert!(
        datei.contains("firma/web") && !datei.contains("alt/repo"),
        "{datei}"
    );
    assert!(Config::aus(Some(&datei), |_| None).is_ok());

    // Ohne Neustart sichtbar.
    let h = get(&p.app, "/api/health").await;
    assert_eq!(h["endpunkt"]["url"], "http://localhost:4000");
    assert_eq!(h["endpunkt"]["modell"], "claude-sonnet-5-5");
    // Auch die älteren Namen der Anfrage (`litellm`) gelten weiter.
    let (status, antwort) = put(
        &p.app,
        "/api/konfiguration",
        r#"{"litellm":{"url":"localhost:4001"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{antwort}");
    let e = get(&p.app, "/api/einrichtung").await;
    assert_eq!(e["endpunkt_url"], "http://localhost:4001");
    assert_eq!(e["endpunkt_modell"], "claude-sonnet-5-5");
    assert_eq!(e["anbieter"], "claude-agent-sdk");
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
        serde_json::from_str::<ConfigResponse>(&erste)
            .unwrap()
            .hint
            .is_none()
    );
    let (_, zweite) = put(&p.app, "/api/konfiguration", &id("neu")).await;
    assert!(
        serde_json::from_str::<ConfigResponse>(&zweite)
            .unwrap()
            .hint
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

// ---- Verbindungstest für Jira (nur wiremock, nie ein echter Dienst) ----

use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::config::JiraKonfig;

const TOKEN: &str = "test-wert-123";

/// Ein Server, dessen Jira-Adresse `site` ist; der Token liegt nur auf Wunsch im Schlüsselbund.
fn jira_stand(site: &str, email: &str, mit_token: bool) -> (Router, Arc<MemoryStore>) {
    let secrets = Arc::new(MemoryStore::new());
    if mit_token {
        secrets.set("jira", &Secret::new(TOKEN)).unwrap();
    }
    let quellen = Quellen {
        jira: (!site.is_empty()).then(|| JiraKonfig {
            site: site.into(),
            email: email.into(),
            projects: vec![],
            jql: None,
        }),
        ..Quellen::default()
    };
    let state = AppState::neu(
        db::open_im_speicher().unwrap(),
        secrets.clone(),
        8787,
        quellen,
        Einstellungen::default(),
    );
    (router(state, PathBuf::from("/x")), secrets)
}

async fn pruefen(app: &Router) -> serde_json::Value {
    let (status, body) = sende(
        app,
        anfrage("POST", "/api/jira/pruefen")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(TOKEN), "Token in der Antwort: {body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn jira_test_ok_liest_nur_und_kennt_den_namen() {
    let jira = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .and(header_exists("authorization"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"accountId":"acc-1","displayName":"Mira Keller"}"#),
        )
        .mount(&jira)
        .await;
    let (app, _) = jira_stand(&jira.uri(), "mira@firma.example", true);
    let a = pruefen(&app).await;
    assert_eq!(a["ok"], true);
    assert_eq!(a["ursache"], "ok");
    assert_eq!(a["name"], "Mira Keller");
    assert!(a["meldung"].as_str().unwrap().contains("kennt dich"));
    // Genau ein lesender Aufruf, sonst nichts.
    let anfragen = jira.received_requests().await.unwrap();
    assert_eq!(anfragen.len(), 1);
    assert_eq!(anfragen[0].method.as_str(), "GET");
    assert_eq!(anfragen[0].url.path(), "/rest/api/3/myself");
}

#[tokio::test]
async fn jira_test_sagt_im_klartext_woran_es_liegt() {
    for (antwort, ursache) in [
        (ResponseTemplate::new(401), "token"),
        (ResponseTemplate::new(403), "berechtigung"),
        (ResponseTemplate::new(404), "adresse"),
        // Eine fremde Seite statt Jira.
        (
            ResponseTemplate::new(200).set_body_string("<html>Anmelden</html>"),
            "adresse",
        ),
        (ResponseTemplate::new(500), "sonst"),
    ] {
        let jira = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/myself"))
            .respond_with(antwort)
            .mount(&jira)
            .await;
        let (app, _) = jira_stand(&jira.uri(), "mira@firma.example", true);
        let a = pruefen(&app).await;
        assert_eq!(a["ok"], false);
        assert_eq!(a["ursache"], ursache, "{a}");
        assert!(!a["meldung"].as_str().unwrap().is_empty());
    }
}

#[tokio::test]
async fn jira_test_ohne_verbindung_und_ohne_angaben() {
    // Nichts lauscht auf dem Port.
    let (app, _) = jira_stand("http://127.0.0.1:1", "mira@firma.example", true);
    assert_eq!(pruefen(&app).await["ursache"], "erreichbar");
    // Ohne Token, ohne E-Mail, ohne Site: „Angaben“, ohne dass etwas aufgerufen wird.
    for (site, email, token) in [
        ("http://127.0.0.1:1", "mira@firma.example", false),
        ("http://127.0.0.1:1", "", true),
        ("", "", true),
    ] {
        let (app, _) = jira_stand(site, email, token);
        let a = pruefen(&app).await;
        assert_eq!(a["ursache"], "angaben", "{site} {email} {token}");
        assert_eq!(a["ok"], false);
    }
}

#[tokio::test]
async fn jira_test_gibt_den_token_nie_aus_auch_wenn_jira_ihn_zurueckspielt() {
    let jira = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_string(format!("Bad {TOKEN}")))
        .mount(&jira)
        .await;
    let (app, _) = jira_stand(&jira.uri(), "mira@firma.example", true);
    let a = pruefen(&app).await;
    assert_eq!(a["ok"], false);
    // `pruefen` prüft schon, dass der Token nicht in der Antwort steht.
    assert!(!a.to_string().contains(TOKEN));
}

#[tokio::test]
async fn trennen_loescht_nur_tokens() {
    let (app, secrets) = jira_stand("", "", true);
    secrets.set("litellm", &Secret::new(TOKEN)).unwrap();
    let loeschen = |name: &str| {
        anfrage("DELETE", &format!("/api/schluessel/{name}"))
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap()
    };
    let (status, body) = sende(&app, loeschen("jira")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""gesetzt":false"#) && !body.contains(TOKEN));
    assert!(secrets.get("jira").unwrap().is_none());
    // Zweites Mal ist kein Fehler.
    assert_eq!(sende(&app, loeschen("jira")).await.0, StatusCode::OK);
    // Der Modell-Key und Client-Secrets bleiben.
    for name in ["litellm", "atlassian-oauth", "unsinn"] {
        assert_eq!(sende(&app, loeschen(name)).await.0, StatusCode::NOT_FOUND);
    }
    assert!(secrets.get("litellm").unwrap().is_some());
    // Ohne JSON-Content-Type lehnt die gemeinsame Prüfung ab.
    let roh = anfrage("DELETE", "/api/schluessel/jira")
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(&app, roh).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}
