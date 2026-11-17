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
