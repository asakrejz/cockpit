//! Jira attachments: the client reads them with its key, with a size limit, only for numeric
//! IDs, and a redirect to the media service does not carry the key along.

use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::{Error, Secret};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
// base64("mira@firma.example:jira-token")
const BASIC: &str = "Basic bWlyYUBmaXJtYS5leGFtcGxlOmppcmEtdG9rZW4=";

fn client(s: &MockServer) -> JiraClient {
    JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jira-token")).unwrap()
}

#[tokio::test]
async fn reads_content_and_thumbnail_with_the_key() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/attachment/content/10001"))
        .and(header("authorization", BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_raw(PNG, "image/png"))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/attachment/thumbnail/10001"))
        .and(header("authorization", BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_raw(&PNG[..8], "image/png"))
        .mount(&s)
        .await;
    let c = client(&s);
    let full = c.attachment("10001", false, 1024).await.unwrap();
    assert_eq!(full.bytes, PNG);
    assert_eq!(full.content_type.as_deref(), Some("image/png"));
    assert_eq!(
        c.attachment("10001", true, 1024).await.unwrap().bytes.len(),
        8
    );
}

#[tokio::test]
async fn a_file_over_the_limit_is_too_large() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/attachment/content/10001"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 2000]))
        .mount(&s)
        .await;
    let c = client(&s);
    assert_eq!(
        c.attachment("10001", false, 1000).await.unwrap_err(),
        Error::TooLarge
    );
    assert_eq!(
        c.attachment("10001", false, 2000)
            .await
            .unwrap()
            .bytes
            .len(),
        2000
    );
}

#[tokio::test]
async fn only_digits_go_into_the_address() {
    let s = MockServer::start().await;
    // Nothing is mounted: a request would answer 404 – but none must be sent at all.
    let c = client(&s);
    for id in ["", "../x", "1/2", "10001?x=1", "abc"] {
        assert_eq!(
            c.attachment(id, false, 10).await.unwrap_err(),
            Error::NotFound,
            "{id:?}"
        );
    }
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn jira_errors_become_errors() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/attachment/content/1"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/attachment/content/2"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&s)
        .await;
    let c = client(&s);
    assert_eq!(
        c.attachment("1", false, 10).await.unwrap_err(),
        Error::NotFound
    );
    assert_eq!(
        c.attachment("2", false, 10).await.unwrap_err(),
        Error::NotSignedIn
    );
}

#[tokio::test]
async fn the_key_is_not_sent_to_another_host_by_a_redirect() {
    // Jira answers with a redirect to the media service; that one is another host (here: the
    // same machine on another name), so the key must not follow.
    let media = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(PNG, "image/png"))
        .mount(&media)
        .await;
    let jira = MockServer::start().await;
    let other_name = media.uri().replace("127.0.0.1", "localhost");
    Mock::given(method("GET"))
        .and(path("/rest/api/3/attachment/content/5"))
        .respond_with(
            ResponseTemplate::new(303).insert_header("location", format!("{other_name}/file")),
        )
        .mount(&jira)
        .await;
    let got = client(&jira).attachment("5", false, 1024).await.unwrap();
    assert_eq!(got.bytes, PNG);
    let seen = media.received_requests().await.unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        !seen[0].headers.contains_key("authorization"),
        "key leaked to the media host"
    );
}
