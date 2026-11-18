//! Review panel: commits and commit diffs against replayed responses. No network access.

use cockpit_core::LineKind;
use cockpit_core::api::PANEL_FILE_LINES;
use cockpit_integrations::Secret;
use cockpit_integrations::github::dto::{CommitDetail, PullFile};
use cockpit_integrations::github::{GitHubClient, panel_commits, panel_diff};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn answer(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/json")
}

async fn client(server: &MockServer) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new("ghp_testtoken")).unwrap()
}

#[tokio::test]
async fn commits_are_listed_with_title_body_and_author() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/pulls/87/commits"))
        .and(header("authorization", "Bearer ghp_testtoken"))
        .respond_with(answer(include_str!("fixtures/github_commits_87.json")))
        .expect(1)
        .mount(&server)
        .await;
    let list = client(&server)
        .await
        .commits("acme/baukasten", 87)
        .await
        .unwrap();
    let panel = panel_commits(&list);
    assert!(!panel.truncated);
    assert_eq!(panel.commits.len(), 2);
    assert_eq!(panel.commits[0].title, "Use the avatar token");
    assert_eq!(
        panel.commits[0].body,
        "The size now comes from --comp-avatar-size."
    );
    assert_eq!(panel.commits[0].author, "mkeller");
    assert_eq!(panel.commits[0].date, "2024-05-13T09:30:00Z");
    // No GitHub account: the name in the commit.
    assert_eq!(panel.commits[1].author, "Jo Doe");
    assert_eq!(panel.commits[1].body, "");
}

#[tokio::test]
async fn commit_diff_has_only_the_files_of_that_commit() {
    let server = MockServer::start().await;
    let sha = "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111";
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/baukasten/commits/{sha}")))
        .respond_with(answer(include_str!("fixtures/github_commit_aaaa.json")))
        .expect(1)
        .mount(&server)
        .await;
    let detail = client(&server)
        .await
        .commit("acme/baukasten", sha)
        .await
        .unwrap();
    let diff = panel_diff(&detail, None);
    assert_eq!(diff.sha, sha);
    assert_eq!(diff.files.len(), 2);
    let css = &diff.files[0];
    assert_eq!((css.additions, css.deletions), (2, 2));
    assert_eq!(css.status, "modified");
    assert!(!css.no_patch && !css.truncated);
    assert_eq!(
        css.lines.iter().filter(|l| l.kind == LineKind::Add).count(),
        2
    );
    // A binary file has no patch.
    assert!(diff.files[1].no_patch);
    assert!(diff.files[1].lines.is_empty());
}

fn long_file(name: &str, lines: usize) -> PullFile {
    let mut patch = format!("@@ -0,0 +1,{lines} @@\n");
    for i in 0..lines {
        patch.push_str(&format!("+line {i}\n"));
    }
    PullFile {
        filename: name.into(),
        status: "added".into(),
        additions: lines as u32,
        deletions: 0,
        patch: Some(patch),
    }
}

#[test]
fn long_diffs_are_cut_and_can_be_fetched_per_file() {
    let detail = CommitDetail {
        sha: "abc".into(),
        files: vec![long_file("a.txt", 1000), long_file("b.txt", 5)],
    };
    let cut = panel_diff(&detail, None);
    assert_eq!(cut.files[0].lines.len(), PANEL_FILE_LINES);
    assert_eq!(cut.files[0].total_lines, 1000);
    assert!(cut.files[0].truncated);
    assert!(!cut.files[1].truncated);

    let full = panel_diff(&detail, Some("a.txt"));
    assert_eq!(full.files.len(), 1);
    assert_eq!(full.files[0].lines.len(), 1000);
    assert!(!full.files[0].truncated);

    let nothing = panel_diff(&detail, Some("missing.txt"));
    assert!(nothing.files.is_empty());
}

#[test]
fn many_files_are_cut_and_flagged() {
    let detail = CommitDetail {
        sha: "abc".into(),
        files: (0..320)
            .map(|i| long_file(&format!("f{i}.txt"), 1))
            .collect(),
    };
    let diff = panel_diff(&detail, None);
    assert_eq!(diff.files.len(), 300);
    assert!(diff.truncated_files);
}
