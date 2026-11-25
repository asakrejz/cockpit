//! The review run of the real agent: fake `docker`, a local Git remote, no real service. The clone
//! must be mounted read-only, the container must get no GitHub token, nothing may be pushed, and
//! the drafts are checked against the diff.

use super::followup::pr_welt;
use super::*;
use cockpit_core::{DiffSide, PrKey};
use wiremock::MockServer;

const KEY: &str = "prreview-firma~app-85";

/// An added line of the first file of #85: `(file, line number)`.
fn spot(w: &World) -> (String, u32) {
    let ws = w.ws();
    let f = &ws.review(85).unwrap().files[0];
    let l = f
        .lines
        .iter()
        .find(|l| l.kind == cockpit_core::LineKind::Add)
        .unwrap();
    (f.name.clone(), l.no)
}

/// The "container": copies what it got, reports two drafts (one good, one for a file that is not
/// in the diff), a summary and the result.
fn scenario_review(file: &str, line: u32) -> String {
    format!(
        r#"for a in "$@"; do case "$a" in *:/job:ro) job="${{a%%:/job:ro}}";; esac; done
cp "$job/job.json" "$d/job-copy.json"
env > "$d/env-all.log"
printf '%s\n' \
 '{{"t":"step","title":"Datei lesen","text":"{file}","tokens_in":100,"tokens_out":20,"ms":500}}' \
 '{{"t":"review_comment","path":"{file}","line":{line},"side":"RIGHT","text":"Das bitte prüfen","suggestion":"neu();"}}' \
 '{{"t":"review_comment","path":"gibt/es/nicht.rs","line":1,"side":"RIGHT","text":"erfunden"}}' \
 '{{"t":"review_comment","path":"{file}","line":{line},"side":"MITTE","text":"falsche Seite"}}' \
 '{{"t":"review_summary","text":"Sieht ordentlich aus."}}' \
 '{{"t":"result","text":"1 Kommentar als Entwurf vorbereitet.","branch":"feature/liste","commit":""}}'
exit 0
"#
    )
}

fn world(g: &MockServer) -> World {
    let w = pr_welt(g);
    // The pull request is on a branch that is not `claude/…`: reviews read any branch.
    let source = w.tmp.path().join("source");
    sh(&source, &["checkout", "--quiet", "-b", "feature/liste"]);
    std::fs::write(source.join("liste.txt"), "fremd\n").unwrap();
    sh(&source, &["add", "-A"]);
    sh(&source, &["commit", "--quiet", "-m", "Fremder Branch"]);
    let remote = w.remote();
    sh(
        &source,
        &["push", "--quiet", remote.to_str().unwrap(), "feature/liste"],
    );
    sh(&source, &["checkout", "--quiet", "main"]);
    let mut ws = w.ws();
    for r in ws.reviews.iter_mut().filter(|r| r.number == 85) {
        r.branch = "feature/liste".into();
    }
    w.state.inner.live.set_state_for_test(ws);
    w
}

#[tokio::test]
async fn a_review_run_reads_only_and_keeps_its_drafts_local() {
    let g = MockServer::start().await;
    let w = world(&g);
    let (file, line) = spot(&w);
    w.write("scenario-plan.sh", &scenario_review(&file, line));
    let branches_before = w.branches();

    w.action(Action::StartPrReview {
        review: PrKey::new("firma/app", 85),
        language: None,
        repo_language: None,
    })
    .await
    .unwrap();
    w.wait("review finished", |ws| {
        ws.review(85).is_some_and(|r| r.agent_reviewed)
    })
    .await;
    w.wait_for_end().await;

    // Read-only: the clone is mounted `:ro`, never writable.
    let calls = w.log("calls.log");
    let run_line = calls.lines().find(|l| l.starts_with("run")).unwrap();
    assert!(run_line.contains(":/work:ro"), "{run_line}");
    assert!(!run_line.contains(":/work "), "{run_line}");
    // No GitHub token anywhere: not in the arguments, not in the environment, not in the job.
    for (name, text) in [
        ("arguments", calls.clone()),
        ("environment", w.log("env-all.log")),
        ("env.log", w.log("env.log")),
        ("job", w.log("job-copy.json")),
    ] {
        assert!(!text.contains(GITHUB), "token in {name}");
    }
    assert!(!w.log("env-all.log").contains("GITHUB_TOKEN"));
    // The job is a review job with the diff as data.
    let job: serde_json::Value = serde_json::from_str(&w.log("job-copy.json")).unwrap();
    assert_eq!(job["mode"], "review");
    assert_eq!(job["review"]["pull_request"], 85);
    assert_eq!(job["review"]["files"][0]["name"], file);
    assert_eq!(job["branch"], "feature/liste");
    // Nothing was pushed.
    assert_eq!(w.branches(), branches_before);
    // Only the draft that fits the diff was kept; the summary is stored, everything stays local.
    let ws = w.ws();
    let r = ws.review(85).unwrap();
    let drafts: Vec<_> = r.suggestions.iter().collect();
    assert_eq!(drafts.len(), 1, "{drafts:#?}");
    assert_eq!(
        (drafts[0].path.as_str(), drafts[0].line, drafts[0].side),
        (file.as_str(), line, DiffSide::Right)
    );
    assert_eq!(drafts[0].code.as_deref(), Some("neu();"));
    assert_eq!(drafts[0].decision, None);
    assert_eq!(r.agent_summary, "Sieht ordentlich aus.");
    assert!(!r.sent);
    assert_eq!(r.verdict, cockpit_core::Verdict::Comment);
    // The mock GitHub got no request at all.
    assert!(g.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_review_run_in_an_unknown_repo_needs_github_to_show_the_repo() {
    let g = MockServer::start().await;
    let w = world(&g);
    let mut ws = w.ws();
    for l in ws.sync.prs.iter_mut().filter(|l| l.number == 85) {
        l.repo = "fremd/repo".into();
    }
    for r in ws.reviews.iter_mut().filter(|r| r.number == 85) {
        r.repo = "fremd/repo".into();
    }
    w.state.inner.live.set_state_for_test(ws);
    let err = w
        .action(Action::StartPrReview {
            review: PrKey::new("fremd/repo", 85),
            language: None,
            repo_language: None,
        })
        .await
        .unwrap_err();
    // The repo has no settings yet: they are read from GitHub first, which does not show it.
    assert!(err.contains("GitHub kennt das Repo"), "{err}");
    assert!(w.ws().runs.iter().all(|r| r.key() != KEY));
}

#[tokio::test]
async fn a_failing_container_ends_the_run_with_a_reason_and_no_drafts() {
    let g = MockServer::start().await;
    let w = world(&g);
    w.write(
        "scenario-plan.sh",
        "printf '%s\\n' '{\"t\":\"error\",\"text\":\"Modell nicht erreichbar\"}'\nexit 1\n",
    );
    w.action(Action::StartPrReview {
        review: PrKey::new("firma/app", 85),
        language: None,
        repo_language: None,
    })
    .await
    .unwrap();
    w.wait("run ended", |ws| {
        ws.runs
            .iter()
            .any(|r| r.key() == KEY && r.is_idle() && r.ended.is_some())
    })
    .await;
    let ws = w.ws();
    let run = ws.runs.iter().find(|r| r.key() == KEY).unwrap();
    assert!(german(&run.ended).contains("Modell nicht erreichbar"));
    assert!(!ws.review(85).unwrap().agent_reviewed);
}

#[tokio::test]
async fn the_branch_pattern_does_not_block_a_read_only_review() {
    let g = MockServer::start().await;
    let w = world(&g);
    // The repo names agent branches `fix/…`; the pull request is on `feature/liste` (not that).
    {
        let mut c = w.state.inner.agent_repo_config.write().unwrap();
        c.repos = vec![cockpit_core::AgentRepo {
            branch_pattern: "fix/{ticket}-{slug}".into(),
            ..cockpit_core::AgentRepo::new("firma/app")
        }];
    }
    let (file, line) = spot(&w);
    w.write("scenario-plan.sh", &scenario_review(&file, line));
    let branches_before = w.branches();
    w.action(Action::StartPrReview {
        review: PrKey::new("firma/app", 85),
        language: None,
        repo_language: None,
    })
    .await
    .unwrap();
    w.wait("review finished", |ws| {
        ws.review(85).is_some_and(|r| r.agent_reviewed)
    })
    .await;
    w.wait_for_end().await;
    // The review ran on the foreign branch, and still nothing was pushed.
    assert!(w.ws().review(85).unwrap().agent_reviewed);
    assert_eq!(w.branches(), branches_before);
}
