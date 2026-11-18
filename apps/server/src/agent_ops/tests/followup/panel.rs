//! Review panel: commits and commit diffs come from GitHub on demand, only for known pull requests.

use super::*;

#[tokio::test]
async fn commits_and_a_commit_diff_are_read_from_github() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/commits"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {GITHUB}").as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "sha": "abcdef1234567",
            "commit": {"message": "Titel\n\nText", "author": {"name": "Mira", "date": "2026-01-01T10:00:00Z"}},
            "author": {"login": "mira"}
        }])))
        .expect(1)
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/commits/abcdef1234567"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abcdef1234567",
            "files": [{
                "filename": "liste.txt", "status": "modified", "additions": 1, "deletions": 0,
                "patch": "@@ -1 +1,2 @@\n alt\n+neu"
            }]
        })))
        .expect(1)
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    let commits = w.state.panel_commits("firma/app", 85).await.unwrap();
    assert_eq!(commits.commits[0].title, "Titel");
    assert_eq!(commits.commits[0].author, "mira");
    let diff = w
        .state
        .panel_commit_diff("firma/app", 85, "abcdef1234567", None)
        .await
        .unwrap();
    assert_eq!(diff.files[0].name, "liste.txt");
    assert_eq!(diff.files[0].lines.len(), 2);
    // Read only: nothing but GET.
    assert!(
        github
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() == "GET")
    );
}

#[tokio::test]
async fn an_unknown_pull_request_is_not_asked_at_github() {
    let github = MockServer::start().await;
    let w = pr_welt(&github);
    let e = w
        .state
        .panel_commits("firma/app", 999)
        .await
        .unwrap_err()
        .de();
    assert!(e.contains("nicht bekannt"), "{e}");
    let e = w
        .state
        .panel_commits("other/repo", 85)
        .await
        .unwrap_err()
        .de();
    assert!(e.contains("nicht bekannt"), "{e}");
    assert!(github.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_github_error_becomes_a_sentence_without_the_token() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/commits"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    let e = w
        .state
        .panel_commits("firma/app", 85)
        .await
        .unwrap_err()
        .de();
    assert!(e.contains("404") && !e.contains(GITHUB), "{e}");
}
