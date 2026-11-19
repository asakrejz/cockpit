//! Write calls against a mock server: right method, path, body and auth header;
//! error cases; the key never shows up in messages. None of this has been checked against
//! the real services.

use cockpit_core::{MergeMethod, TicketStatus};
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::outbound::{LineComment, Outbound, ReviewData};
use cockpit_integrations::{Error, GitHubService, JiraService, Secret, Services};
use serde_json::json;
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "ghp_sehr_geheim_123";

async fn github(server: &MockServer) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new(TOKEN)).unwrap()
}

async fn jira(server: &MockServer) -> JiraClient {
    JiraClient::new(&server.uri(), "ich@firma.example", Secret::new(TOKEN)).unwrap()
}

fn bearer() -> impl wiremock::Match {
    header("authorization", format!("Bearer {TOKEN}").as_str())
}

fn review() -> ReviewData {
    ReviewData {
        verdict: "REQUEST_CHANGES",
        text: "Bitte nachbessern".into(),
        comments: vec![
            LineComment {
                path: "a.rs".into(),
                line: 12,
                page: "RIGHT",
                start_line: None,
                text: "Besser so\n\n```suggestion\n    neu();\n```".into(),
            },
            LineComment {
                path: "a.rs".into(),
                line: 3,
                page: "LEFT",
                start_line: None,
                text: "weg".into(),
            },
        ],
        sha: "abc123".into(),
    }
}

// ---------- GitHub ----------

#[tokio::test]
async fn github_comment() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/issues/7/comments"))
        .and(bearer())
        .and(header("x-github-api-version", "2022-11-28"))
        .and(body_json(json!({"body": "Hallo"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .execute(&Outbound::GitHubComment {
            repo: "a/x".into(),
            number: 7,
            text: "Hallo".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_review_with_positioned_comments_and_suggestion() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/pulls/7/reviews"))
        .and(bearer())
        .and(body_json(json!({
            "event": "REQUEST_CHANGES",
            "body": "Bitte nachbessern",
            "commit_id": "abc123",
            "comments": [
                {"path": "a.rs", "line": 12, "side": "RIGHT",
                 "body": "Besser so\n\n```suggestion\n    neu();\n```"},
                {"path": "a.rs", "line": 3, "side": "LEFT", "body": "weg"}
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 9})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .execute(&Outbound::GitHubReview {
            repo: "a/x".into(),
            number: 7,
            review: review(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_merge_with_method() {
    let s = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/repos/a/x/pulls/7/merge"))
        .and(bearer())
        .and(body_json(
            json!({"merge_method": "squash", "sha": "abc123"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"merged": true})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .execute(&Outbound::GitHubMerge {
            repo: "a/x".into(),
            number: 7,
            method: MergeMethod::Squash,
            sha: "abc123".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_branch_from_default_branch() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "trunk"})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/git/ref/heads/trunk"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"object": {"sha": "deadbeef"}})),
        )
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/git/refs"))
        .and(bearer())
        .and(body_json(
            json!({"ref": "refs/heads/feature/ABC-1-x", "sha": "deadbeef"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .execute(&Outbound::GitHubBranch {
            repo: "a/x".into(),
            name: "feature/ABC-1-x".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_rerequest_check() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 7, "title": "t", "user": {"login": "x"},
            "head": {"ref": "f", "sha": "cafe"}, "base": {"ref": "main"}
        })))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/commits/cafe/check-runs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 11, "name": "Lint", "status": "completed", "conclusion": "success"},
                {"id": 22, "name": "Tests", "status": "completed", "conclusion": "failure"}
            ]})),
        )
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/check-runs/22/rerequest"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&s)
        .await;
    let c = github(&s).await;
    c.execute(&Outbound::GitHubRecheck {
        repo: "a/x".into(),
        number: 7,
        check: "Tests".into(),
    })
    .await
    .unwrap();
    // A check that does not exist is reported by the client in an understandable way.
    let error = c
        .check_again_start("a/x", 7, "Gibt es nicht")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Gibt es nicht"), "{error}");
}

// ---------- Jira ----------

#[tokio::test]
async fn jira_comment_as_adf() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(header_exists("authorization"))
        .and(body_json(
            json!({"body": {"type": "doc", "version": 1, "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Eins"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Zwei"}]}
            ]}}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": "1"})))
        .expect(1)
        .mount(&s)
        .await;
    jira(&s)
        .await
        .execute(&Outbound::JiraComment {
            key: "ABC-1".into(),
            text: "Eins\nZwei".into(),
        })
        .await
        .unwrap();
    // Basic auth with email and token.
    let request = s.received_requests().await.unwrap();
    let auth = request[0]
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(auth.starts_with("Basic "), "{auth}");
}

fn transitions() -> serde_json::Value {
    json!({"transitions": [
        {"id": "11", "name": "Start", "to": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}},
        {"id": "31", "name": "Erledigt", "to": {"name": "Done", "statusCategory": {"key": "done"}}}
    ]})
}

#[tokio::test]
async fn jira_status_finds_the_matching_transition() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(transitions()))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/ABC-1/transitions"))
        .and(body_json(json!({"transition": {"id": "31"}})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&s)
        .await;
    jira(&s)
        .await
        .execute(&Outbound::JiraStatus {
            key: "ABC-1".into(),
            status: TicketStatus::Done,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn jira_without_matching_transition_calls_nothing() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(transitions()))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&s)
        .await;
    let error = jira(&s)
        .await
        .status_set("ABC-1", TicketStatus::Review)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Jira bietet für ABC-1 keine Transition nach „Review“ an"
    );
}

// ---------- Error cases ----------

async fn comment_with(response: ResponseTemplate) -> Error {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(&s)
        .await;
    github(&s)
        .await
        .execute(&Outbound::GitHubComment {
            repo: "a/x".into(),
            number: 7,
            text: "x".into(),
        })
        .await
        .unwrap_err()
}

#[tokio::test]
async fn errors_401_403_404_422_and_rate_limit() {
    assert_eq!(
        comment_with(ResponseTemplate::new(401)).await,
        Error::NotSignedIn
    );
    assert_eq!(
        comment_with(ResponseTemplate::new(403)).await,
        Error::NoPermission
    );
    assert_eq!(
        comment_with(ResponseTemplate::new(404)).await,
        Error::NotFound
    );
    assert_eq!(
        comment_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("retry-after", "30")
        )
        .await,
        Error::RateLimit { wait_s: Some(30) }
    );
    assert_eq!(
        comment_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": "Validation Failed", "errors": ["Zeile fehlt"]}))
        )
        .await,
        Error::Rejected("Validation Failed; Zeile fehlt".into())
    );
}

#[tokio::test]
async fn merge_is_rejected_with_the_service_message() {
    let s = MockServer::start().await;
    Mock::given(method("PUT"))
        .respond_with(
            ResponseTemplate::new(405)
                .set_body_json(json!({"message": "Pull Request is not mergeable"})),
        )
        .mount(&s)
        .await;
    let error = github(&s)
        .await
        .execute(&Outbound::GitHubMerge {
            repo: "a/x".into(),
            number: 7,
            method: MergeMethod::Merge,
            sha: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Der Dienst lehnt die Anfrage ab: Pull Request is not mergeable"
    );
}

// ---------- Key ----------

#[tokio::test]
async fn key_never_appears_in_errors_or_debug() {
    // A service that echoes the key back in the message.
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": format!("Bad credentials: {TOKEN}")})),
        )
        .mount(&s)
        .await;
    let c = github(&s).await;
    let error = c
        .execute(&Outbound::GitHubComment {
            repo: "a/x".into(),
            number: 1,
            text: "x".into(),
        })
        .await
        .unwrap_err();
    for text in [error.to_string(), format!("{error:?}"), format!("{c:?}")] {
        assert!(!text.contains(TOKEN), "{text}");
    }

    // Network gone.
    let c = GitHubClient::new("http://127.0.0.1:9", Secret::new(TOKEN)).unwrap();
    let error = c
        .execute(&Outbound::GitHubComment {
            repo: "a/x".into(),
            number: 1,
            text: "x".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Network(_)));
    assert!(!error.to_string().contains(TOKEN));

    // Jira: token sits in the Basic header, never in messages.
    let j = JiraClient::new("http://127.0.0.1:9", "a@b.de", Secret::new(TOKEN)).unwrap();
    assert!(!format!("{j:?}").contains(TOKEN));
}

#[tokio::test]
async fn services_route_to_the_right_client() {
    let g = MockServer::start().await;
    let j = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/issues/7/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&g)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&j)
        .await;
    let services = Services {
        github: Some(GitHubService {
            client: github(&g).await,
            repos: vec!["a/x".into()],
        }),
        jira: Some(JiraService {
            client: jira(&j).await,
            jql: "x".into(),
            in_project: false,
            fields: Default::default(),
        }),
    };
    assert!(services.targets().github && services.targets().jira);
    services
        .execute(&Outbound::GitHubComment {
            repo: "a/x".into(),
            number: 7,
            text: "a".into(),
        })
        .await
        .unwrap();
    services
        .execute(&Outbound::JiraComment {
            key: "ABC-1".into(),
            text: "b".into(),
        })
        .await
        .unwrap();
    // Without Jira: an understandable error instead of a crash.
    let github_only = Services {
        jira: None,
        github: None,
    };
    assert!(github_only.is_empty());
    assert!(
        github_only
            .execute(&Outbound::JiraComment {
                key: "ABC-1".into(),
                text: "b".into()
            })
            .await
            .is_err()
    );
}

// ---------- Deleting a branch ----------

#[tokio::test]
async fn deleting_a_branch_is_one_delete_on_that_ref_with_the_token() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/repos/acme/app/git/refs/heads/claude/BAU-1-x"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    github(&server)
        .await
        .branch_delete("acme/app", "claude/BAU-1-x")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_branch_delete_is_an_error_without_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({"message": TOKEN})))
        .mount(&server)
        .await;
    let e = github(&server)
        .await
        .branch_delete("acme/app", "claude/BAU-1-x")
        .await
        .unwrap_err();
    assert!(!e.to_string().contains(TOKEN), "{e}");
}

#[tokio::test]
async fn the_protection_of_a_branch_is_read_without_writing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/branches/claude/BAU-1-x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"protected": true})))
        .mount(&server)
        .await;
    assert!(
        github(&server)
            .await
            .branch_protected("acme/app", "claude/BAU-1-x")
            .await
            .unwrap()
    );
}
