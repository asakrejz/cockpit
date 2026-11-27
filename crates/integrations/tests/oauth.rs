//! OAuth against replayed responses: exchange, refresh, revoke, sites, service errors.

use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::oauth::{
    App, Endpoints, Service, Tokens, code_exchange, jira_base, refresh, revoked, sites,
};
use cockpit_integrations::{Error, Secret};
use wiremock::matchers::{basic_auth, body_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "client-geheim-xyz";

fn endpoints(server: &MockServer) -> Endpoints {
    Endpoints {
        github_sign_in: format!("{}/login/oauth/authorize", server.uri()),
        github_token: format!("{}/login/oauth/access_token", server.uri()),
        atlassian_sign_in: format!("{}/authorize", server.uri()),
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
        expires_in_s: Some(100),
    }
}

#[tokio::test]
async fn github_code_is_exchanged() {
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
    let secret = Secret::new(SECRET);
    let t = code_exchange(
        &endpoints(&server),
        Service::GitHub,
        &app(&secret),
        "der-code",
        "ver",
        1000,
    )
    .await
    .unwrap();
    assert_eq!(t.access.expose(), "ghu_neu");
    assert_eq!(t.refresh.unwrap().expose(), "ghr_neu");
    assert_eq!(t.expires_in_s, Some(1000 + 28800));
}

#[tokio::test]
async fn github_responds_200_with_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_verification_code",
            "error_description": "The code passed is incorrect or expired."
        })))
        .mount(&server)
        .await;
    let secret = Secret::new(SECRET);
    let f = code_exchange(
        &endpoints(&server),
        Service::GitHub,
        &app(&secret),
        "x",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&f, Error::Rejected(m) if m.contains("incorrect or expired")),
        "{f:?}"
    );
}

#[tokio::test]
async fn atlassian_code_is_exchanged_as_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_json(serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": "id123",
            "client_secret": SECRET,
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
    let secret = Secret::new(SECRET);
    let t = code_exchange(
        &endpoints(&server),
        Service::Atlassian,
        &app(&secret),
        "c",
        "v",
        10,
    )
    .await
    .unwrap();
    assert_eq!((t.access.expose(), t.expires_in_s), ("at", Some(3610)));
    assert_eq!(t.refresh.unwrap().expose(), "rt");
}

#[tokio::test]
async fn service_error_response_without_secret() {
    let server = MockServer::start().await;
    // The service echoes the secret back in the message.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": "access_denied",
            "error_description": format!("Unknown or invalid refresh token {SECRET}")
        })))
        .mount(&server)
        .await;
    let secret = Secret::new(SECRET);
    let f = code_exchange(
        &endpoints(&server),
        Service::Atlassian,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(!format!("{f:?} {f}").contains(SECRET));

    // 400 (invalid_grant) comes with the service's message.
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant", "error_description": "Der Code ist abgelaufen"
        })))
        .mount(&server)
        .await;
    let f = code_exchange(
        &endpoints(&server),
        Service::Atlassian,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&f, Error::Rejected(m) if m.contains("abgelaufen")),
        "{f:?}"
    );

    // 401 (wrong client secret).
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let f = code_exchange(
        &endpoints(&server),
        Service::Atlassian,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert_eq!(f, Error::NotSignedIn);

    // Not JSON.
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
        .mount(&server)
        .await;
    let f = code_exchange(
        &endpoints(&server),
        Service::GitHub,
        &app(&secret),
        "c",
        "v",
        0,
    )
    .await
    .unwrap_err();
    assert!(matches!(f, Error::Response(_)));
}

#[tokio::test]
async fn github_refresh_replaces_both_tokens() {
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
    let secret = Secret::new(SECRET);
    let t = refresh(
        &endpoints(&server),
        Service::GitHub,
        &app(&secret),
        &tokens(Some("alt-refresh")),
        5000,
    )
    .await
    .unwrap();
    assert_eq!(t.access.expose(), "ghu_2");
    assert_eq!(t.refresh.unwrap().expose(), "ghr_2");
    assert_eq!(t.expires_in_s, Some(5000 + 28800));
}

#[tokio::test]
async fn atlassian_refresh_rotates_the_refresh_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_json(serde_json::json!({
            "grant_type": "refresh_token", "client_id": "id123",
            "client_secret": SECRET, "refresh_token": "alt-refresh",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "at2", "refresh_token": "rt2", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(SECRET);
    let t = refresh(
        &endpoints(&server),
        Service::Atlassian,
        &app(&secret),
        &tokens(Some("alt-refresh")),
        0,
    )
    .await
    .unwrap();
    assert_eq!(t.refresh.unwrap().expose(), "rt2");
}

#[tokio::test]
async fn refresh_fails() {
    let secret = Secret::new(SECRET);
    // Without a refresh token: not signed in, without a network call.
    let f = refresh(
        &Endpoints::default(),
        Service::GitHub,
        &app(&secret),
        &tokens(None),
        0,
    )
    .await
    .unwrap_err();
    assert_eq!(f, Error::NotSignedIn);

    // Revoked refresh token.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "bad_refresh_token", "error_description": "The refresh token passed is incorrect or expired."
        })))
        .mount(&server)
        .await;
    let f = refresh(
        &endpoints(&server),
        Service::GitHub,
        &app(&secret),
        &tokens(Some("r")),
        0,
    )
    .await
    .unwrap_err();
    assert!(matches!(f, Error::Rejected(_)));
}

#[tokio::test]
async fn github_revoke_calls_the_service() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/applications/id123/token"))
        .and(basic_auth("id123", SECRET))
        .and(body_json(
            serde_json::json!({ "access_token": "alt-access" }),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let secret = Secret::new(SECRET);
    let ok = revoked(
        Service::GitHub,
        &app(&secret),
        &server.uri(),
        &tokens(Some("r")),
    )
    .await
    .unwrap();
    assert!(ok);

    // Already revoked (404) counts as done; other errors are reported.
    server.reset().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    assert!(
        revoked(Service::GitHub, &app(&secret), &server.uri(), &tokens(None))
            .await
            .unwrap()
    );
    server.reset().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    assert_eq!(
        revoked(Service::GitHub, &app(&secret), &server.uri(), &tokens(None))
            .await
            .unwrap_err(),
        Error::Status(500)
    );
}

#[tokio::test]
async fn atlassian_has_no_revoke_call() {
    let secret = Secret::new(SECRET);
    // No server needed: nothing goes out.
    let at_service = revoked(
        Service::Atlassian,
        &app(&secret),
        "https://unbenutzt.invalid",
        &tokens(None),
    )
    .await
    .unwrap();
    assert!(!at_service);
}

#[tokio::test]
async fn sites_and_jira_via_the_gateway() {
    let server = MockServer::start().await;
    let e = endpoints(&server);
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
    let list = sites(&e, &Secret::new("at")).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "1324a887-451a");

    // Jira is addressed via /ex/jira/<cloudId> with Bearer.
    Mock::given(method("GET"))
        .and(path("/ex/jira/1324a887-451a/rest/api/3/myself"))
        .and(header("authorization", "Bearer at"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "accountId": "acc-1", "displayName": "Ich"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let base = jira_base(&e, &list[0].id).unwrap();
    let jira = JiraClient::oauth(&base, Secret::new("at")).unwrap();
    assert_eq!(jira.me().await.unwrap(), "acc-1");
}

#[tokio::test]
async fn revoked_token_becomes_not_signed_in() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "code": 401, "message": "Unauthorized"
        })))
        .mount(&server)
        .await;
    let f = sites(&endpoints(&server), &Secret::new("at"))
        .await
        .unwrap_err();
    assert_eq!(f, Error::NotSignedIn);
    assert_eq!(f.problem(0), cockpit_core::Problem::Unauthorized);
    let jira =
        JiraClient::oauth(&format!("{}/ex/jira/abc", server.uri()), Secret::new("at")).unwrap();
    assert_eq!(jira.me().await.unwrap_err(), Error::NotSignedIn);
}
