use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use cockpit_integrations::oauth::Endpoints;
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

fn endpunkte(s: &MockServer) -> Endpoints {
    Endpoints {
        github_sign_in: format!("{}/login/oauth/authorize", s.uri()),
        github_token: format!("{}/login/oauth/access_token", s.uri()),
        atlassian_sign_in: format!("{}/authorize", s.uri()),
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
    // Ohne Client-Secret in der .env-Datei.
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
    assert!(p.state.oauth_start(Service::Atlassian).await.is_err());

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
    // Tokens liegen in der .env-Datei.
    let gespeichert = p
        .secrets
        .get(secrets::GITHUB_OAUTH_TOKENS)
        .unwrap()
        .unwrap();
    let t = Tokens::from_text(gespeichert.expose()).unwrap();
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
    let a: RevokeResponse = serde_json::from_str(&body).unwrap();
    assert!(a.disconnected && a.at_service, "{body}");
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
    let a = p.state.oauth_widerrufen(Service::GitHub).await.unwrap();
    assert!(a.disconnected && !a.at_service);
    assert!(a.hint.unwrap().contains("nicht bestätigt"));
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
    let a: RevokeResponse = serde_json::from_str(&body).unwrap();
    assert!(a.disconnected && !a.at_service);
    assert!(a.hint.unwrap().contains("id.atlassian.com"));
    assert!(!p.secrets.has(secrets::ATLASSIAN_OAUTH_TOKENS).unwrap());
    assert_eq!(p.state.atlassian_site(), None);
    // Widerruf ohne Anmeldung ist kein Fehler.
    assert!(
        p.state
            .oauth_widerrufen(Service::Atlassian)
            .await
            .unwrap()
            .disconnected
    );
}

fn abgelaufen(p: &Prufstand, d: Service, refresh: Option<&str>) {
    let t = Tokens {
        access: Secret::new("alt_zugriff"),
        refresh: refresh.map(Secret::new),
        expires_in_s: Some(1),
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
    abgelaufen(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_auffrischen().await;
    let t = p.state.oauth_tokens(Service::GitHub).unwrap();
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
    abgelaufen(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_auffrischen().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_none());
    let s = p.state.dienst_status(Service::GitHub, false);
    assert!(!s.connected);
    assert_eq!(s.kind, AuthKind::None);

    // Ohne Refresh-Token ist der abgelaufene Zugriff nicht zu retten.
    abgelaufen(&p, Service::GitHub, None);
    p.state.oauth_auffrischen().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_none());
}

#[tokio::test]
async fn stoerung_des_dienstes_trennt_nicht() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let p = stand(&server, None, true);
    abgelaufen(&p, Service::GitHub, Some("alt_erneuern"));
    p.state.oauth_auffrischen().await;
    assert!(p.state.oauth_tokens(Service::GitHub).is_some());
}

#[tokio::test]
async fn oauth_geht_vor_dem_api_token() {
    let server = MockServer::start().await;
    let p = stand(&server, None, true);
    p.secrets
        .set(secrets::GITHUB, &Secret::new("ghp_api_token_123"))
        .unwrap();
    let status = p.state.dienst_status(Service::GitHub, true);
    assert_eq!((status.connected, status.kind), (true, AuthKind::Token));
    p.state
        .oauth_tokens_setzen(
            Service::GitHub,
            &Tokens {
                access: Secret::new("ghu_x"),
                refresh: None,
                expires_in_s: None,
            },
        )
        .unwrap();
    let status = p.state.dienst_status(Service::GitHub, true);
    assert_eq!(status.kind, AuthKind::OAuth);
    assert!(status.oauth_possible);
    // Nach dem Trennen gilt wieder das Token.
    p.state.oauth_widerrufen(Service::GitHub).await.unwrap();
    assert_eq!(
        p.state.dienst_status(Service::GitHub, true).kind,
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
        redirect_uri(Service::GitHub, 8787),
        "http://127.0.0.1:8787/api/oauth/github/callback"
    );
    assert_eq!(
        redirect_uri(Service::Atlassian, 8787),
        "http://localhost:8787/api/oauth/atlassian/callback"
    );
    assert_eq!(kodiert("Ä b&c=d#"), "%C3%84%20b%26c%3Dd%23");
}
