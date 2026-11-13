//! Follow-up jobs and the pull request of the real agent: fake `docker`, a local Git remote and a
//! mock server as GitHub. Nothing here talks to a real service.

use super::*;
use crate::config::GitHubConfig;
use cockpit_core::api::PullRequestPreview;
use cockpit_core::{FollowupTask, PrLink, PullRequestRef};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

#[tokio::test]
async fn review_comments_run_on_the_same_branch_and_push_normally() {
    let github = MockServer::start().await;
    comments_ok(&github).await;
    let w = pr_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::AddressReviewThreads {
        review: 85,
        options: direct(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.runs
            .iter()
            .any(|r| r.key() == "threads-85" && r.is_done())
    })
    .await;
    w.wait_for_end().await;

    // New commit on the branch of the pull request, history kept (normal push).
    let log = remote_log(&w).await;
    assert!(
        log.starts_with("Folge") && log.contains("Agent früher"),
        "{log}"
    );
    assert_eq!(w.branches(), [PR_BRANCH, "main"]);

    // The job file carries the threads, the review text and the branch – nothing secret.
    let job: serde_json::Value =
        serde_json::from_str(&w.log("job-copy.json")).expect("job.json wurde kopiert");
    assert_eq!(job["branch"], PR_BRANCH);
    assert_eq!(job["followup"]["kind"], "review_comments");
    assert_eq!(job["followup"]["pull_request"], 85);
    assert_eq!(
        job["followup"]["threads"][0]["comments"][1]["text"],
        "Mach ich"
    );
    assert_eq!(job["followup"]["notes"][0]["text"], "Noch Tests");
    let text = w.log("job-copy.json");
    assert!(!text.contains(GITHUB) && !text.contains(TOKEN), "{text}");

    // Security: the container process got the model key, but never the GitHub token.
    let env = w.log("env-all.log");
    assert!(!env.contains(GITHUB), "GitHub-Token im Container: {env}");
    let calls = w.log("calls.log");
    assert!(!calls.contains(GITHUB) && !calls.contains(TOKEN), "{calls}");
    assert!(!env.contains("GITHUB_TOKEN") && !env.contains("GH_TOKEN"));

    // The report looks like that of a normal run.
    let run = w
        .ws()
        .runs
        .into_iter()
        .find(|r| r.key() == "threads-85")
        .unwrap();
    assert!(run.summary.contains("Einzug angepasst."));
    assert!(run.usage().input_tokens >= 100);
    assert_eq!(run.branch(), PR_BRANCH);
    assert!(w.ws().log.iter().all(|e| !e.text.contains(GITHUB)));
}

#[tokio::test]
async fn failed_checks_go_to_the_agent_with_a_shortened_excerpt() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 85, "title": "t", "user": {"login": "ich"},
            "head": {"ref": PR_BRANCH, "sha": "abc123"}, "base": {"ref": "main"}
        })))
        .mount(&github)
        .await;
    let long = format!("{}FEHLER-AM-ENDE", "x".repeat(20_000));
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/commits/abc123/check-runs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 7, "name": "Lint", "status": "completed", "conclusion": "failure",
                 "output": {"title": "rot", "summary": long}},
                {"id": 8, "name": "Build", "status": "completed", "conclusion": "success"}
            ]})),
        )
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/check-runs/7/annotations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::FixFailedChecks {
        review: 85,
        options: direct(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.runs.iter().any(|r| r.key() == "fixes-85" && r.is_done())
    })
    .await;
    w.wait_for_end().await;
    let job: serde_json::Value = serde_json::from_str(&w.log("job-copy.json")).unwrap();
    assert_eq!(job["followup"]["kind"], "failed_checks");
    let checks = job["followup"]["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["name"], "Lint");
    let excerpt = checks[0]["excerpt"].as_str().unwrap();
    assert!(excerpt.chars().count() <= 3_000, "{}", excerpt.len());
    assert!(excerpt.ends_with("FEHLER-AM-ENDE"));
    assert!(remote_log(&w).await.starts_with("Folge"));
}

#[tokio::test]
async fn secrets_in_review_texts_do_not_reach_the_job_file() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "user": {"login": "x"}, "body": format!("Mein Token ist {GITHUB}"), "path": "a", "line": 1}
        ])))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::AddressReviewThreads {
        review: 85,
        options: direct(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.runs
            .iter()
            .any(|r| r.key() == "threads-85" && r.is_done())
    })
    .await;
    w.wait_for_end().await;
    let text = w.log("job-copy.json");
    assert!(text.contains("Mein Token ist ***"), "{text}");
    assert!(!text.contains(GITHUB));
}

#[tokio::test]
async fn nothing_to_do_is_refused_before_a_container_starts() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    let e = w
        .action(Action::AddressReviewThreads {
            review: 85,
            options: direct(),
        })
        .await
        .unwrap_err();
    assert!(e.contains("keine Review-Kommentare"), "{e}");
    assert!(w.ws().runs.iter().all(|r| r.key() != "threads-85"));
    assert!(!w.log("calls.log").contains("run "));
}

#[tokio::test]
async fn github_errors_on_the_inputs_are_told_in_plain_words() {
    for (status, needle) in [(401, "401"), (403, "403")] {
        let github = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"message": "x"})))
            .mount(&github)
            .await;
        let w = pr_welt(&github);
        let e = w
            .action(Action::AddressReviewThreads {
                review: 85,
                options: direct(),
            })
            .await
            .unwrap_err();
        assert!(e.contains(needle) && e.contains("Review-Kommentare"), "{e}");
        assert!(!e.contains(GITHUB));
    }
}

#[tokio::test]
async fn followups_need_the_real_agent_to_be_ready_and_an_agent_branch() {
    // Not set up: the message says what is missing.
    let github = MockServer::start().await;
    let w = pr_welt(&github);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let e = w
        .action(Action::FixFailedChecks {
            review: 85,
            options: direct(),
        })
        .await
        .unwrap_err();
    assert!(
        e.contains("noch nicht starten") && e.contains("Einrichten"),
        "{e}"
    );
    // A person's branch is refused by the rules of the workspace.
    let w2 = pr_welt(&github);
    let mut ws = w2.ws();
    for r in ws.reviews.iter_mut().filter(|r| r.number == 85) {
        r.branch = "feature/x".into();
    }
    w2.state.inner.live.set_state_for_test(ws);
    comments_ok(&github).await;
    let e = w2
        .action(Action::AddressReviewThreads {
            review: 85,
            options: direct(),
        })
        .await
        .unwrap_err();
    assert!(e.contains("claude/"), "{e}");
}

// ---------- Pull request ----------

/// A finished real run of BAU-142: branch pushed to the local remote.
async fn done_world(github: &MockServer) -> World {
    let w = pr_welt(github);
    w.state.inner.live.set_state_for_test(demo::workspace());
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    w
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

async fn none_open(github: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(github)
        .await;
}

#[tokio::test]
async fn preview_shows_title_text_and_creates_nothing() {
    let github = MockServer::start().await;
    none_open(&github).await;
    let w = done_world(&github).await;
    let p: PullRequestPreview = w.state.pull_request_preview("BAU-142").await.unwrap();
    assert!(p.title.starts_with("BAU-142: "));
    assert!(p.body.contains("Fokus-Ring ergänzt."));
    assert_eq!((p.head.as_str(), p.base.as_str()), (BRANCH, "main"));
    assert_eq!(p.repo, "firma/app");
    assert_eq!(p.existing, None);
    let posts = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .count();
    assert_eq!(posts, 0, "die Vorschau legt nichts an");
    assert!(w.run().pull_request.is_none(), "und ändert nichts");
}

#[tokio::test]
async fn the_pull_request_is_created_only_on_the_action_and_with_the_edited_text() {
    let github = MockServer::start().await;
    none_open(&github).await;
    Mock::given(method("POST"))
        .and(path("/repos/firma/app/pulls"))
        .and(wiremock::matchers::body_partial_json(json!({
            "title": "BAU-142: Mein Titel", "head": BRANCH, "base": "main", "body": "Text", "draft": true
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 31, "html_url": "https://github.example/firma/app/pull/31", "state": "open"
        })))
        .expect(1)
        .mount(&github)
        .await;
    let w = done_world(&github).await;
    // Finishing the run alone creates nothing.
    assert!(
        github
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "POST")
    );
    w.action(create("BAU-142: Mein Titel")).await.unwrap();
    let run = w.run();
    let pull = run.pull_request.clone().unwrap();
    assert_eq!(
        (pull.number, pull.existed),
        (31, false),
        "GitHub entscheidet, nicht der Browser"
    );
    assert_eq!(pull.url, "https://github.example/firma/app/pull/31");
    assert_eq!(run.pr_number, Some(31));
    let entry = w.ws().log.last().cloned().unwrap();
    assert_eq!(entry.actor, Actor::User);
    assert!(entry.text.contains("#31"));
    // Nothing else was called: no merge, no review request, no PATCH.
    let others: Vec<String> = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| !matches!(r.method.as_str(), "GET" | "POST"))
        .map(|r| format!("{} {}", r.method, r.url.path()))
        .collect();
    assert!(others.is_empty(), "{others:?}");
    // A second click does not create a duplicate.
    let e = w.action(create("BAU-142: Mein Titel")).await.unwrap_err();
    assert!(e.contains("#31"), "{e}");
}

#[tokio::test]
async fn an_existing_pull_request_is_linked_instead_of_duplicated() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"number": 12, "html_url": "https://github.example/firma/app/pull/12", "state": "open",
             "head": {"ref": BRANCH, "sha": "abc"}}
        ])))
        .mount(&github)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&github)
        .await;
    let w = done_world(&github).await;
    let p = w.state.pull_request_preview("BAU-142").await.unwrap();
    assert_eq!(p.existing.as_ref().map(|e| e.number), Some(12));
    w.action(create("BAU-142: x")).await.unwrap();
    let pull = w.run().pull_request.unwrap();
    assert_eq!((pull.number, pull.existed), (12, true));
    assert!(w.ws().log.last().unwrap().text.contains("schon"));
}

#[tokio::test]
async fn a_pull_request_created_in_the_meantime_is_found_after_the_422() {
    let github = MockServer::start().await;
    // First look-up: nothing; after the failed creation: theirs.
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .up_to_n_times(1)
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"number": 40, "html_url": "u", "state": "open", "head": {"ref": BRANCH, "sha": "a"}}
        ])))
        .mount(&github)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Validation Failed",
            "errors": [{"message": "A pull request already exists for firma:claude/x"}]
        })))
        .mount(&github)
        .await;
    let w = done_world(&github).await;
    // The preview uses up the first (empty) answer.
    let _ = w.state.pull_request_preview("BAU-142").await.unwrap();
    w.action(create("BAU-142: x")).await.unwrap();
    assert_eq!(w.run().pull_request.unwrap().number, 40);
}

#[tokio::test]
async fn errors_of_github_are_told_in_plain_words_and_change_nothing() {
    for (status, body, needle) in [
        (401, json!({"message": "Bad credentials"}), "401"),
        (403, json!({"message": "Resource not accessible"}), "403"),
        (
            422,
            json!({"message": "Validation Failed", "errors": [{"message": "No commits between main and x"}]}),
            "No commits between",
        ),
    ] {
        let github = MockServer::start().await;
        none_open(&github).await;
        Mock::given(method("POST"))
            .and(path("/repos/firma/app/pulls"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&github)
            .await;
        let w = done_world(&github).await;
        let before = w.ws();
        let e = w.action(create("BAU-142: x")).await.unwrap_err();
        assert!(e.contains(needle) && e.contains("nichts geändert"), "{e}");
        assert!(!e.contains(GITHUB));
        assert_eq!(w.run().pull_request, None);
        assert_eq!(w.ws().log.len(), before.log.len());
    }
}

#[tokio::test]
async fn no_pull_request_before_the_run_is_done_or_with_an_empty_title() {
    let github = MockServer::start().await;
    none_open(&github).await;
    let w = pr_welt(&github);
    w.state.inner.live.set_state_for_test(demo::workspace());
    let e = w.action(create("BAU-142: x")).await.unwrap_err();
    assert!(!e.is_empty());
    let w = done_world(&github).await;
    let e = w.action(create("  ")).await.unwrap_err();
    assert!(e.contains("Eingabe"), "{e}");
    assert!(
        github
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method.as_str() != "POST")
    );
}

#[tokio::test]
async fn the_simulation_refuses_followups_and_pull_requests() {
    let w = welt(true);
    let mut settings = w.state.inner.settings.clone();
    settings.seed = Seed::Demo;
    let conn = crate::db::open(&w.tmp.path().join("sim.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        settings,
    );
    state.inner.live.set_state_for_test(demo::workspace());
    for a in [
        Action::AddressReviewThreads {
            review: 85,
            options: direct(),
        },
        create("x"),
    ] {
        let e = state.action_run(a).await;
        assert!(e.is_err());
    }
    let _ = FollowupTask::ReviewThreads;
}
