//! OAuth gegen nachgebaute Antworten: Austausch, Refresh, Widerruf, Sites, Fehler des Dienstes.
//! Nichts davon ist gegen die echten Dienste geprüft.

use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::{
    App, Dienst, Endpunkte, Tokens, auffrischen, code_tauschen, jira_basis, sites, widerrufen,
};
use cockpit_integrations::{Fehler, Secret};
use wiremock::matchers::{basic_auth, body_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const GEHEIM: &str = "client-geheim-xyz";

fn endpunkte(server: &MockServer) -> Endpunkte {
    Endpunkte {
        github_anmeldung: format!("{}/login/oauth/authorize", server.uri()),
        github_token: format!("{}/login/oauth/access_token", server.uri()),
        atlassian_anmeldung: format!("{}/authorize", server.uri()),
        atlassian_token: format!("{}/oauth/token", server.uri()),
        atlassian_api: server.uri(),
    }
}

fn app(secret: &Secret) -> App<'_> {
    App {
        client_id: "id123",
        client_secret: secret,
        redirect_uri: "http://127.0.0.1:8787/api/oauth/github/callback",
    }
}

fn tokens(refresh: Option<&str>) -> Tokens {
    Tokens {
        access: Secret::new("alt-access"),
        refresh: refresh.map(Secret::new),
        laeuft_ab_s: Some(100),
    }
}

#[tokio::test]
async fn github_code_wird_getauscht() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .and(body_string_contains("code=der-code"))
        .and(body_string_contains("code_verifier=ver"))
        .and(body_string_contains("client_secret=client-geheim-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ghu_neu", "expires_in": 28800,
            "refresh_token": "ghr_neu", "refresh_token_expires_in": 15811200,
            "token_type": "bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let t = code_tauschen(
        &endpunkte(&server),
        Dienst::GitHub,
        &app(&secret),
        "der-code",
        "ver",
        1000,
    )
    .await
    .unwrap();
    assert_eq!(t.access.expose(), "ghu_neu");
    assert_eq!(t.refresh.unwrap().expose(), "ghr_neu");
    assert_eq!(t.laeuft_ab_s, Some(1000 + 28800));
}

#[tokio::test]
async fn github_antwortet_200_mit_fehler() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_verification_code",
            "error_description": "The code passed is incorrect or expired."
        })))
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let f = code_tauschen(
        &endpunkte(&server),
        Dienst::GitHub,
        &app(&secret),
        "x",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&f, Fehler::Abgelehnt(m) if m.contains("incorrect or expired")),
        "{f:?}"
    );
}

#[tokio::test]
async fn atlassian_code_wird_als_json_getauscht() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_json(serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": "id123",
            "client_secret": GEHEIM,
            "code": "c",
            "redirect_uri": "http://127.0.0.1:8787/api/oauth/github/callback",
            "code_verifier": "v",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "at", "refresh_token": "rt", "expires_in": 3600,
            "scope": "read:jira-work offline_access", "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let t = code_tauschen(
        &endpunkte(&server),
        Dienst::Atlassian,
        &app(&secret),
        "c",
        "v",
        10,
    )
    .await
    .unwrap();
    assert_eq!((t.access.expose(), t.laeuft_ab_s), ("at", Some(3610)));
    assert_eq!(t.refresh.unwrap().expose(), "rt");
}

#[tokio::test]
async fn fehlerantwort_des_dienstes_ohne_geheimnis() {
    let server = MockServer::start().await;
    // Der Dienst spielt das Secret in der Meldung zurück.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": "access_denied",
            "error_description": format!("Unknown or invalid refresh token {GEHEIM}")
        })))
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let f = code_tauschen(
        &endpunkte(&server),
        Dienst::Atlassian,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(!format!("{f:?} {f}").contains(GEHEIM));

    // 400 (invalid_grant) kommt mit der Meldung des Dienstes.
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant", "error_description": "Der Code ist abgelaufen"
        })))
        .mount(&server)
        .await;
    let f = code_tauschen(
        &endpunkte(&server),
        Dienst::Atlassian,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&f, Fehler::Abgelehnt(m) if m.contains("abgelaufen")),
        "{f:?}"
    );

    // 401 (falsches Client-Secret).
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let f = code_tauschen(
        &endpunkte(&server),
        Dienst::Atlassian,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert_eq!(f, Fehler::NichtAngemeldet);

    // Kein JSON.
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
        .mount(&server)
        .await;
    let f = code_tauschen(
        &endpunkte(&server),
        Dienst::GitHub,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(matches!(f, Fehler::Antwort(_)));
}

#[tokio::test]
async fn github_refresh_ersetzt_beide_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=alt-refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "ghu_2", "expires_in": 28800, "refresh_token": "ghr_2"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let t = auffrischen(
        &endpunkte(&server),
        Dienst::GitHub,
        &app(&secret),
        &tokens(Some("alt-refresh")),
        5000,
    )
    .await
    .unwrap();
    assert_eq!(t.access.expose(), "ghu_2");
    assert_eq!(t.refresh.unwrap().expose(), "ghr_2");
    assert_eq!(t.laeuft_ab_s, Some(5000 + 28800));
}

#[tokio::test]
async fn atlassian_refresh_dreht_das_refresh_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_json(serde_json::json!({
            "grant_type": "refresh_token", "client_id": "id123",
            "client_secret": GEHEIM, "refresh_token": "alt-refresh",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "at2", "refresh_token": "rt2", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let t = auffrischen(
        &endpunkte(&server),
        Dienst::Atlassian,
        &app(&secret),
        &tokens(Some("alt-refresh")),
        0,
    )
    .await
    .unwrap();
    assert_eq!(t.refresh.unwrap().expose(), "rt2");
}

#[tokio::test]
async fn refresh_scheitert() {
    let secret = Secret::new(GEHEIM);
    // Ohne Refresh-Token: nicht angemeldet, ohne Netzwerkaufruf.
    let f = auffrischen(
        &Endpunkte::default(),
        Dienst::GitHub,
        &app(&secret),
        &tokens(None),
        0,
    )
    .await
    .unwrap_err();
    assert_eq!(f, Fehler::NichtAngemeldet);

    // Widerrufenes Refresh-Token.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_refresh_token", "error_description": "The refresh token passed is incorrect or expired."
        })))
        .mount(&server)
        .await;
    let f = auffrischen(
        &endpunkte(&server),
        Dienst::GitHub,
        &app(&secret),
        &tokens(Some("r")),
        0,
    )
    .await
    .unwrap_err();
    assert!(matches!(f, Fehler::Abgelehnt(_)));
}

#[tokio::test]
async fn github_widerruf_ruft_den_dienst() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/applications/id123/token"))
        .and(basic_auth("id123", GEHEIM))
        .and(body_json(
            serde_json::json!({ "access_token": "alt-access" }),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(GEHEIM);
    let ok = widerrufen(
        Dienst::GitHub,
        &app(&secret),
        &server.uri(),
        &tokens(Some("r")),
    )
    .await
    .unwrap();
    assert!(ok);

    // Schon widerrufen (404) zählt als erledigt; andere Fehler werden gemeldet.
    server.reset().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    assert!(
        widerrufen(Dienst::GitHub, &app(&secret), &server.uri(), &tokens(None))
            .await
            .unwrap()
    );
    server.reset().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    assert_eq!(
        widerrufen(Dienst::GitHub, &app(&secret), &server.uri(), &tokens(None))
            .await
            .unwrap_err(),
        Fehler::Status(500)
    );
}

#[tokio::test]
async fn atlassian_hat_keinen_widerrufs_aufruf() {
    let secret = Secret::new(GEHEIM);
    // Kein Server nötig: es geht nichts nach außen.
    let beim_dienst = widerrufen(
        Dienst::Atlassian,
        &app(&secret),
        "https://unbenutzt.invalid",
        &tokens(None),
    )
    .await
    .unwrap();
    assert!(!beim_dienst);
}

#[tokio::test]
async fn sites_und_jira_ueber_das_gateway() {
    let server = MockServer::start().await;
    let e = endpunkte(&server);
    Mock::given(method("GET"))
        .and(path("/oauth/token/accessible-resources"))
        .and(header("authorization", "Bearer at"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "id": "1324a887-451a", "name": "firma",
                "url": "https://firma.atlassian.net",
                "scopes": ["read:jira-work"], "avatarUrl": "x"
            }])),
        )
        .expect(1)
        .mount(&server)
        .await;
    let liste = sites(&e, &Secret::new("at")).await.unwrap();
    assert_eq!(liste.len(), 1);
    assert_eq!(liste[0].id, "1324a887-451a");

    // Jira wird über /ex/jira/<cloudId> mit Bearer angesprochen.
    Mock::given(method("GET"))
        .and(path("/ex/jira/1324a887-451a/rest/api/3/myself"))
        .and(header("authorization", "Bearer at"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "accountId": "acc-1", "displayName": "Ich"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let basis = jira_basis(&e, &liste[0].id).unwrap();
    let jira = JiraClient::oauth(&basis, Secret::new("at")).unwrap();
    assert_eq!(jira.ich().await.unwrap(), "acc-1");
}

#[tokio::test]
async fn widerrufenes_token_wird_zu_nicht_angemeldet() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "code": 401, "message": "Unauthorized"
        })))
        .mount(&server)
        .await;
    let f = sites(&endpunkte(&server), &Secret::new("at"))
        .await
        .unwrap_err();
    assert_eq!(f, Fehler::NichtAngemeldet);
    assert_eq!(f.problem(0), cockpit_core::Problem::Unauthorized);
    let jira =
        JiraClient::oauth(&format!("{}/ex/jira/abc", server.uri()), Secret::new("at")).unwrap();
    assert_eq!(jira.ich().await.unwrap_err(), Fehler::NichtAngemeldet);
}
