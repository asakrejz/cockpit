//! Follow-up jobs and the pull request of the real agent: fake `docker`, a local Git remote and a
//! mock server as GitHub. Nothing here talks to a real service.

use super::*;
use crate::config::GitHubConfig;
use cockpit_core::api::PullRequestPreview;
use cockpit_core::{FollowupTask, PrLink, PullRequestRef};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod jobs;
mod pull_request;
mod two_repos;

const PR_BRANCH: &str = "claude/BAU-127-list-separator";

/// The world plus a pull request #85 on an agent branch that exists in the remote.
fn pr_welt(github: &MockServer) -> World {
    let w = welt(true);
    // The branch exists in the remote with one commit of the "earlier run".
    let source = w.tmp.path().join("source");
    sh(&source, &["checkout", "--quiet", "-b", PR_BRANCH]);
    std::fs::write(source.join("liste.txt"), "alt\n").unwrap();
    sh(&source, &["add", "-A"]);
    sh(&source, &["commit", "--quiet", "-m", "Agent früher"]);
    let remote = w.remote();
    sh(
        &source,
        &["push", "--quiet", remote.to_str().unwrap(), PR_BRANCH],
    );
    sh(&source, &["checkout", "--quiet", "main"]);

    let mut ws = demo::workspace();
    for r in ws.reviews.iter_mut().filter(|r| r.number == 85) {
        r.branch = PR_BRANCH.into();
        r.ticket_key = None;
        r.repo = "firma/app".into();
    }
    for p in ws
        .items
        .iter_mut()
        .filter_map(|i| i.pr.as_mut())
        .filter(|p| p.number == 85)
    {
        p.repo = "firma/app".into();
    }
    ws.sync.prs.push(PrLink {
        number: 85,
        repo: "firma/app".into(),
        head_sha: "abc123".into(),
    });
    w.state.inner.live.set_state_for_test(ws);
    let sources = crate::sync::Sources {
        github: Some(GitHubConfig {
            api_url: github.uri(),
            repos: vec!["firma/app".into()],
        }),
        ..crate::sync::Sources::default()
    };
    *w.state.inner.sources.write().unwrap() = Arc::new(sources);
    w
}

fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    }
}

async fn comments_ok(s: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "user": {"login": "mira"}, "body": "Bitte Einzug prüfen", "path": "liste.txt", "line": 1},
            {"id": 2, "in_reply_to_id": 1, "user": {"login": "ich"}, "body": "Mach ich", "path": "liste.txt", "line": 1},
        ])))
        .mount(s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"user": {"login": "mira"}, "state": "CHANGES_REQUESTED", "body": "Noch Tests", "submitted_at": "2026-01-01T10:00:00Z"}
        ])))
        .mount(s)
        .await;
}

/// The "container" of a follow-up job: keeps copies of what it got, commits and reports.
fn scenario_followup() -> String {
    format!(
        r#"for a in "$@"; do case "$a" in *:/job:ro) job="${{a%%:/job:ro}}";; esac; done
cp "$job/job.json" "$d/job-copy.json"
env > "$d/env-all.log"
echo neu >> "$clone/liste.txt"
git -C "$clone" add -A
git -C "$clone" -c user.name=a -c user.email=a@a.invalid commit -qm Folge
printf '%s\n' \
 '{{"t":"step","title":"Datei ändern","text":"liste.txt","tokens_in":100,"tokens_out":20,"ms":500}}' \
 '{{"t":"files","files":["liste.txt"]}}' \
 '{{"t":"result","text":"Einzug angepasst.","branch":"{PR_BRANCH}","commit":"abc1234"}}'
exit 0
"#
    )
}

async fn remote_log(w: &World) -> String {
    let out = std::process::Command::new("git")
        .current_dir(w.remote())
        .args(["log", "--format=%s", PR_BRANCH])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn create(title: &str) -> Action {
    Action::CreatePullRequest {
        run: "BAU-142".into(),
        title: title.into(),
        body: "Text".into(),
        // Whatever the browser claims is ignored.
        pull: Some(PullRequestRef {
            number: 999,
            url: "https://evil.example".into(),
            existed: true,
        }),
    }
}
