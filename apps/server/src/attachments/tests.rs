use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use cockpit_core::{Attachment, Details};
use cockpit_integrations::Secret;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::app::router;
use crate::config::JiraConfig;
use crate::db;
use crate::secrets::{self, MemoryStore, SecretStore};
use crate::sync::Sources;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

fn attachment(id: &str, name: &str, mime: &str, thumbnail: bool) -> Attachment {
    Attachment {
        id: id.into(),
        name: name.into(),
        size: 11,
        mime: mime.into(),
        thumbnail,
    }
}

/// A server whose Jira is `jira`, with a ticket that has these attachments.
fn state(jira: &MockServer) -> AppState {
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::JIRA, &Secret::new("jira-token"))
        .unwrap();
    let sources = Sources {
        jira: Some(JiraConfig {
            site: jira.uri(),
            email: "mira@acme.example".into(),
            projects: vec![],
            jql: None,
            hidden_fields: vec![],
            field_order: vec![],
            team_field: None,
        }),
        ..Sources::default()
    };
    let state = AppState::with_sources(db::open_in_memory().unwrap(), secrets, 8787, sources);
    let mut ws = state.inner.live.copy();
    let ticket = ws.items.iter_mut().find(|i| i.key == "BAU-142").unwrap();
    ticket.details = Some(Details {
        fields: vec![],
        attachments: vec![
            attachment("10001", "screen.png", "image/png", true),
            attachment("10002", "spec.pdf", "application/pdf", false),
            attachment("10003", "logo.svg", "image/svg+xml", false),
            attachment("10004", "fake.png", "image/png", false),
            attachment("10005", "huge.png", "image/png", false),
        ],
    });
    state.inner.live.set_state_for_test(ws);
    state
}

async fn get_as(state: &AppState, host: &str, uri: &str) -> axum::response::Response {
    let req = Request::builder()
        .uri(uri)
        .header("host", host)
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
        .body(Body::empty())
        .unwrap();
    router(state.clone(), PathBuf::from("/x"))
        .oneshot(req)
        .await
        .unwrap()
}

async fn get(state: &AppState, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let answer = get_as(state, "127.0.0.1:8787", uri).await;
    let (status, headers) = (answer.status(), answer.headers().clone());
    let body = answer
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

async fn serve_file(s: &MockServer, which: &str, id: &str, body: &[u8]) {
    Mock::given(method("GET"))
        .and(path(format!("/rest/api/3/attachment/{which}/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(s)
        .await;
}

#[tokio::test]
async fn a_picture_comes_inline_with_safe_headers_and_is_cached() {
    let jira = MockServer::start().await;
    serve_file(&jira, "content", "10001", PNG).await;
    let state = state(&jira);

    let (status, h, body) = get(&state, "/api/jira/attachment/10001").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(h["content-disposition"], "inline");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    // The key never goes to the browser.
    assert!(
        h.values()
            .all(|v| !v.to_str().unwrap_or("").contains("jira-token"))
    );

    // The second time it comes from the cache.
    let (status, _, again) = get(&state, "/api/jira/attachment/10001").await;
    assert_eq!((status, again), (StatusCode::OK, PNG.to_vec()));
    assert_eq!(jira.received_requests().await.unwrap().len(), 1);
    assert_eq!(state.inner.attachments.size(), PNG.len());
}

#[tokio::test]
async fn the_thumbnail_is_its_own_file() {
    let jira = MockServer::start().await;
    serve_file(&jira, "thumbnail", "10001", &PNG[..8]).await;
    let state = state(&jira);
    let (status, h, body) = get(&state, "/api/jira/attachment/10001/thumbnail").await;
    assert_eq!((status, body.len()), (StatusCode::OK, 8));
    assert_eq!(h["content-type"], "image/png");
    // A file without a thumbnail at Jira has none here either – and Jira is not asked.
    let (status, _, _) = get(&state, "/api/jira/attachment/10002/thumbnail").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(jira.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_id_that_is_not_in_the_workspace_is_404_and_jira_is_not_asked() {
    let jira = MockServer::start().await;
    serve_file(&jira, "content", "99999", PNG).await;
    let state = state(&jira);
    for id in ["99999", "0", "10001x", "1%2F2", "..", "10001/../../x"] {
        let (status, _, _) = get(&state, &format!("/api/jira/attachment/{id}")).await;
        assert!(status == StatusCode::NOT_FOUND, "{id}: {status}");
    }
    assert!(jira.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_file_that_is_too_large_is_413() {
    let jira = MockServer::start().await;
    serve_file(&jira, "content", "10005", &vec![0x89; MAX_BYTES + 1]).await;
    let state = state(&jira);
    let (status, _, _) = get(&state, "/api/jira/attachment/10005").await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(state.inner.attachments.size(), 0, "nothing cached");
}

#[tokio::test]
async fn what_is_no_picture_is_a_download() {
    let jira = MockServer::start().await;
    serve_file(&jira, "content", "10002", b"%PDF-1.7 ...").await;
    serve_file(&jira, "content", "10003", b"<svg onload=alert(1)></svg>").await;
    // Named `.png`, typed as a picture – but the bytes are a web page.
    serve_file(
        &jira,
        "content",
        "10004",
        b"<html><script>alert(1)</script>",
    )
    .await;
    let state = state(&jira);
    for (id, name) in [
        ("10002", "spec.pdf"),
        ("10003", "logo.svg"),
        ("10004", "fake.png"),
    ] {
        let (status, h, _) = get(&state, &format!("/api/jira/attachment/{id}")).await;
        assert_eq!(status, StatusCode::OK, "{id}");
        assert_eq!(h["content-type"], "application/octet-stream", "{id}");
        assert_eq!(
            h["content-disposition"],
            format!("attachment; filename=\"{name}\""),
            "{id}"
        );
        assert_eq!(h["x-content-type-options"], "nosniff");
    }
}

#[tokio::test]
async fn only_from_this_computer() {
    let jira = MockServer::start().await;
    serve_file(&jira, "content", "10001", PNG).await;
    let state = state(&jira);
    let answer = get_as(&state, "evil.example", "/api/jira/attachment/10001").await;
    assert_eq!(answer.status(), StatusCode::FORBIDDEN);
    assert!(jira.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn without_jira_it_is_an_error_not_a_crash() {
    let secrets = Arc::new(MemoryStore::new());
    let state = AppState::with_sources(
        db::open_in_memory().unwrap(),
        secrets,
        8787,
        Sources::default(),
    );
    let mut ws = state.inner.live.copy();
    ws.items[0].details = Some(Details {
        fields: vec![],
        attachments: vec![attachment("7", "a.png", "image/png", false)],
    });
    state.inner.live.set_state_for_test(ws);
    let (status, _, _) = get(&state, "/api/jira/attachment/7").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[test]
fn pictures_are_recognized_by_their_first_bytes() {
    assert_eq!(sniff_image(PNG), Some("image/png"));
    assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
    assert_eq!(sniff_image(b"GIF89a.."), Some("image/gif"));
    assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    assert_eq!(
        sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
        None
    );
    assert_eq!(sniff_image(b"<html>"), None);
    assert_eq!(sniff_image(b""), None);
}

#[test]
fn file_names_are_safe_in_a_header() {
    assert_eq!(safe_file_name("screen.png"), "screen.png");
    assert_eq!(safe_file_name("a\"b\r\nc/d.png"), "a_b__c_d.png");
    assert_eq!(safe_file_name("Übersicht.pdf"), "_bersicht.pdf");
    assert_eq!(safe_file_name("..."), "attachment");
    assert_eq!(safe_file_name(""), "attachment");
    assert!(safe_file_name(&"x".repeat(500)).len() <= 120);
}

#[test]
fn the_cache_stays_below_its_bound() {
    let cache = Cache::default();
    let file = |n: usize| Stored {
        bytes: Arc::new(vec![0; n]),
        image: None,
    };
    let third = CACHE_BYTES / 3 + 1;
    for id in ["a", "b", "c"] {
        cache.put((id.into(), false), file(third));
    }
    // The oldest made room for the newest.
    assert!(cache.get(&("a".into(), false)).is_none());
    assert!(cache.get(&("b".into(), false)).is_some() && cache.get(&("c".into(), false)).is_some());
    assert!(cache.size() <= CACHE_BYTES);
    // A file bigger than the whole cache is not kept at all.
    cache.put(("big".into(), false), file(CACHE_BYTES + 1));
    assert!(cache.get(&("big".into(), false)).is_none());
}

#[tokio::test]
async fn the_sample_picture_comes_without_jira_and_nothing_else_does() {
    let state = AppState::with_sources(
        db::open_in_memory().unwrap(),
        Arc::new(MemoryStore::new()),
        8787,
        Sources::default(),
    );
    // The sample data of the demo ticket points at it.
    let ws = state.inner.live.copy();
    let ticket = ws.items.iter().find(|i| i.key == "BAU-142").unwrap();
    let id = cockpit_core::details::DEMO_IMAGE_ID;
    assert!(
        ticket
            .details
            .as_ref()
            .unwrap()
            .attachments
            .iter()
            .any(|a| a.id == id)
    );
    let (status, h, body) = get(&state, &format!("/api/jira/attachment/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(sniff_image(&body), Some("image/png"));
    // The other sample file has no content.
    let (status, _, _) = get(&state, "/api/jira/attachment/900002").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

/// What the browser is told for a failed download: a file that is too large or unknown says so, and
/// every other failure of Jira is a bad gateway – also a refused key, since the request of the
/// browser itself was fine.
#[tokio::test]
async fn a_failed_download_is_answered_with_the_matching_status() {
    use cockpit_core::{Right, TokenKind};
    let others = [
        (Error::NotSignedIn, StatusCode::BAD_GATEWAY),
        (Error::NoPermission, StatusCode::BAD_GATEWAY),
        (Error::RateLimit { wait_s: None }, StatusCode::BAD_GATEWAY),
        (Error::Network("down".into()), StatusCode::BAD_GATEWAY),
        (Error::Rejected(String::new()), StatusCode::BAD_GATEWAY),
        (Error::Status(500), StatusCode::BAD_GATEWAY),
        (Error::Response("x".into()), StatusCode::BAD_GATEWAY),
        (Error::Config("x".into()), StatusCode::BAD_GATEWAY),
        (Error::NotPossible("x".into()), StatusCode::BAD_GATEWAY),
        (
            Error::Coded(msg!(IntegrationsNotFound)),
            StatusCode::BAD_GATEWAY,
        ),
        (
            Error::MissingRight {
                right: Right::IssuesRead,
                kind: TokenKind::App,
            },
            StatusCode::BAD_GATEWAY,
        ),
    ];
    let table = [
        (Error::TooLarge, StatusCode::PAYLOAD_TOO_LARGE),
        (Error::NotFound, StatusCode::NOT_FOUND),
    ]
    .into_iter()
    .chain(others);
    for (e, status) in table {
        let text = e.msg();
        let answer = failure(e.clone());
        assert_eq!(answer.status(), status, "{e:?}");
        let body = answer.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["msg"], serde_json::to_value(&text).unwrap(), "{e:?}");
    }
}
