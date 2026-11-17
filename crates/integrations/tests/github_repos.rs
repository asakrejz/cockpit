//! GitHub repo lists (`GET /user/repos`, installations) against a local mock server.
//! No network access to real services.

use cockpit_integrations::github::{GitHubClient, MAX_REPOS};
use cockpit_integrations::{Error, Secret};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new("ghp_testtoken")).unwrap()
}

fn entry(name: &str) -> Value {
    json!({"full_name": name, "private": true, "archived": false, "default_branch": "develop",
           "permissions": {"push": true}})
}

fn page(n: usize, from: usize) -> Value {
    Value::Array(
        (from..from + n)
            .map(|i| entry(&format!("acme/r{i:04}")))
            .collect(),
    )
}

#[tokio::test]
async fn token_list_has_all_fields_and_sends_affiliation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/repos"))
        .and(header("authorization", "Bearer ghp_testtoken"))
        .and(query_param(
            "affiliation",
            "owner,collaborator,organization_member",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"full_name": "acme/zeta", "private": false, "archived": true},
            entry("acme/app"),
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let list = client(&server).repo_list().await.unwrap();
    assert!(!list.truncated);
    assert_eq!(list.repos.len(), 2);
    // Sorted by name; missing fields get plain defaults.
    assert_eq!(list.repos[0].repo, "acme/app");
    assert_eq!(list.repos[0].default_branch, "develop");
    assert_eq!(list.repos[0].can_push, Some(true));
    assert!(list.repos[0].private);
    assert_eq!(list.repos[1].default_branch, "main");
    assert_eq!(list.repos[1].can_push, None);
    assert!(list.repos[1].archived && !list.repos[1].private);
}

#[tokio::test]
async fn token_list_pages_through_and_stops_at_the_limit() {
    let server = MockServer::start().await;
    for p in 1..=6usize {
        Mock::given(method("GET"))
            .and(path("/user/repos"))
            .and(query_param("page", p.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(100, (p - 1) * 100)))
            .mount(&server)
            .await;
    }
    let list = client(&server).repo_list().await.unwrap();
    assert_eq!(list.repos.len(), MAX_REPOS);
    assert!(list.truncated);
}

#[tokio::test]
async fn token_list_with_two_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/repos"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(100, 0)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user/repos"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(23, 100)))
        .mount(&server)
        .await;
    let list = client(&server).repo_list().await.unwrap();
    assert_eq!(list.repos.len(), 123);
    assert!(!list.truncated);
}

#[tokio::test]
async fn installation_list_merges_installations() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/installations"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"total_count": 2, "installations": [{"id": 7}, {"id": 9}]})),
        )
        .mount(&server)
        .await;
    for (id, name) in [(7, "acme/b"), (9, "other/a")] {
        Mock::given(method("GET"))
            .and(path(format!("/user/installations/{id}/repositories")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"total_count": 1, "repositories": [entry(name)]})),
            )
            .mount(&server)
            .await;
    }
    let list = client(&server).installation_repo_list().await.unwrap();
    let names: Vec<_> = list.repos.iter().map(|r| r.repo.as_str()).collect();
    assert_eq!(names, ["acme/b", "other/a"]);
}

#[tokio::test]
async fn errors_are_mapped_and_never_contain_the_token() {
    for (status, expect) in [
        (401u16, "NotSignedIn"),
        (403, "NoPermission"),
        (404, "NotFound"),
        (429, "RateLimit"),
        (500, "Status"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/repos"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({"message": "bad ghp_testtoken credentials"})),
            )
            .mount(&server)
            .await;
        let e = client(&server).repo_list().await.unwrap_err();
        let kind = match &e {
            Error::NotSignedIn => "NotSignedIn",
            Error::NoPermission => "NoPermission",
            Error::NotFound => "NotFound",
            Error::RateLimit { .. } => "RateLimit",
            Error::Status(_) => "Status",
            _ => "other",
        };
        assert_eq!(kind, expect, "status {status}");
        assert!(!format!("{e} {e:?}").contains("ghp_testtoken"));
    }
}

fn file(text: &str) -> Value {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD.encode(text);
    // GitHub wraps the base64 text in lines.
    let wrapped: Vec<String> = raw
        .as_bytes()
        .chunks(60)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    json!({"type": "file", "encoding": "base64", "size": text.len(), "content": wrapped.join("\n") + "\n"})
}

#[tokio::test]
async fn contributing_is_read_from_the_first_place_that_has_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/.github/CONTRIBUTING.md"))
        .and(header("authorization", "Bearer ghp_testtoken"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(file("Branches: feature/PROJ-123-short-title. Ünïcode ok.")),
        )
        .expect(1)
        .mount(&server)
        .await;
    // Never asked: the first hit wins.
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/docs/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let (found, text) = client(&server)
        .contributing("acme/app")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found, ".github/CONTRIBUTING.md");
    assert!(text.contains("feature/PROJ-123-short-title") && text.contains("Ünïcode"));
}

#[tokio::test]
async fn contributing_missing_everywhere_is_none_and_only_reads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .expect(3)
        .mount(&server)
        .await;
    assert!(
        client(&server)
            .contributing("acme/app")
            .await
            .unwrap()
            .is_none()
    );
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.method == wiremock::http::Method::GET)
    );
}

#[tokio::test]
async fn contributing_skips_folders_and_huge_files_and_reports_other_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"name": "x"}])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"type": "file", "encoding": "base64", "size": 900_000, "content": "eA=="}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/docs/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"message": "no"})))
        .mount(&server)
        .await;
    let e = client(&server).contributing("acme/app").await.unwrap_err();
    assert!(matches!(e, Error::NoPermission), "{e:?}");
}
