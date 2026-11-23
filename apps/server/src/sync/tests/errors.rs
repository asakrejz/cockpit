//! Errors of Jira and GitHub reach people as catalog messages: the same failure reads right in
//! German and in English. wiremock only.

use super::*;
use cockpit_core::api::ErrorBody;
use cockpit_core::{Problem, Source};
use cockpit_i18n::Lang;

fn error_body(body: &str) -> ErrorBody {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not an error body ({e}): {body}"))
}

/// The message of a response, rendered in both languages (German first).
fn both(body: &str) -> (String, String) {
    let m = error_body(body)
        .msg
        .expect("a catalog message, not a plain text");
    (m.render(Lang::De), m.render(Lang::En))
}

#[tokio::test]
async fn a_failed_sync_is_stored_as_a_message_that_reads_in_both_languages() {
    let g = github().await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    g.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&g)
        .await;
    sync(&state).await;

    let (ws, _) = snapshot(&state).await;
    let problem = ws
        .sync
        .state_of(Source::GitHub)
        .and_then(|s| s.problem.clone())
        .expect("the source has a problem");
    let Problem::Other(message) = &problem else {
        panic!("expected a message, got {problem:?}");
    };
    assert_eq!(message.code, "integrations.server_error");
    assert_eq!(
        message.render(Lang::De),
        "Der Dienst hat einen Serverfehler gemeldet (503)"
    );
    assert_eq!(
        message.render(Lang::En),
        "The service reported a server error (503)"
    );
    // The same message is what the notice for the reader is built from.
    let notice = ws
        .sync
        .state_of(Source::GitHub)
        .and_then(|s| cockpit_core::notice::source_notice(s, 0))
        .expect("a notice");
    assert!(notice.detail.render(Lang::De).contains("Serverfehler"));
    assert!(notice.detail.render(Lang::En).contains("server error"));
}

#[tokio::test]
async fn a_failed_github_lookup_is_a_github_message_in_both_languages() {
    let g = github().await;
    get(&g, "/repos/a/x/assignees", ResponseTemplate::new(503)).await;
    let state = state(sources(&g.uri()), true);
    let req = request("GET", "/api/github/assignable?repo=a/x&query=mi")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_request(&state, req).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        error_body(&body).msg.unwrap().code,
        "server_run.github_no_people"
    );
    let (de, en) = both(&body);
    assert_eq!(
        de,
        "GitHub liefert keine Personen: Der Dienst hat einen Serverfehler gemeldet (503)"
    );
    assert_eq!(
        en,
        "GitHub does not return any people: The service reported a server error (503)"
    );
}

#[tokio::test]
async fn a_github_client_error_is_not_reported_as_a_jira_error() {
    // An address without https cannot be used for a client.
    let state = state(sources("http://insecure.example"), true);
    let req = request("GET", "/api/github/assignable?repo=a/x&query=mi")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_request(&state, req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_body(&body).msg.unwrap().code,
        "server_run.github_error"
    );
    let (de, en) = both(&body);
    assert!(de.starts_with("GitHub: "), "{de}");
    assert!(en.starts_with("GitHub: "), "{en}");
    assert!(!de.contains("Jira") && !en.contains("Jira"), "{de} / {en}");
    assert_ne!(
        de, en,
        "the reason is a message of its own, not one raw text"
    );
}

#[tokio::test]
async fn a_failed_jira_lookup_is_a_jira_message_in_both_languages() {
    let j = MockServer::start().await;
    get(&j, "/rest/api/3/project/search", ResponseTemplate::new(503)).await;
    let state = jira_state(&j.uri());
    let req = request("GET", "/api/jira/projects")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send_request(&state, req).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        error_body(&body).msg.unwrap().code,
        "server_run.jira_no_projects"
    );
    let (de, en) = both(&body);
    assert_eq!(
        de,
        "Jira liefert keine Projekte: Der Dienst hat einen Serverfehler gemeldet (503)"
    );
    assert_eq!(
        en,
        "Jira does not return any projects: The service reported a server error (503)"
    );
}
