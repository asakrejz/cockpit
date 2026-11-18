//! The branch pattern of a repo: set in the configuration, applied at the start, enforced at the push.

use super::*;

fn with_pattern(pattern: &str) -> Vec<cockpit_core::AgentRepo> {
    vec![cockpit_core::AgentRepo {
        branch_pattern: pattern.into(),
        ..cockpit_core::AgentRepo::new("firma/web")
    }]
}

/// The container commits and reports `branch`.
fn scenario_on(branch: &str) -> String {
    scenario_ok().replace(BRANCH, branch)
}

async fn start(w: &World, options: AgentOptions) -> Result<(), String> {
    w.action(Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            plan_first: false,
            ..options
        },
    })
    .await
    .map(|_| ())
}

#[tokio::test]
async fn the_configured_pattern_names_and_pushes_the_branch() {
    let mut w = welt(true);
    // BAU-142 carries the label "bug", so `{type}` is `fix`.
    w.state = with_repos(
        &w,
        with_pattern("{type}/{ticket}-{slug}"),
        Some("firma/web"),
    );
    let branch = "fix/BAU-142-toggle-fokus-ring-fehlt";
    w.write("scenario-run.sh", &scenario_on(branch));
    start(&w, AgentOptions::default()).await.unwrap();
    assert_eq!(w.run().branch(), branch);
    w.wait("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches_of("web"), [branch, "main"]);
}

#[tokio::test]
async fn a_pattern_in_the_request_does_not_count() {
    let mut w = welt(true);
    w.state = with_repos(
        &w,
        with_pattern("feature/{ticket}-{slug}"),
        Some("firma/web"),
    );
    let options = AgentOptions {
        branch_pattern: "main-{ticket}".into(),
        branch_type: "chore".into(),
        ..AgentOptions::default()
    };
    w.write(
        "scenario-run.sh",
        &scenario_on("fix/BAU-142-toggle-fokus-ring-fehlt"),
    );
    start(&w, options).await.unwrap();
    let run = w.run();
    assert_eq!(run.options.branch_pattern, "feature/{ticket}-{slug}");
    // The ticket's own labels decide the type, not the request.
    assert_eq!(run.branch(), "feature/BAU-142-toggle-fokus-ring-fehlt");
    w.wait_for_end().await;
}

#[tokio::test]
async fn a_branch_outside_the_pattern_is_not_pushed() {
    let mut w = welt(true);
    w.state = with_repos(
        &w,
        with_pattern("feature/{ticket}-{slug}"),
        Some("firma/web"),
    );
    // The container reports (and would have committed to) the old default name.
    w.write("scenario-run.sh", &scenario_on(BRANCH));
    start(&w, AgentOptions::default()).await.unwrap();
    w.wait("Lauf endet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches_of("web"), ["main"], "nothing was pushed");
    let ended = w.run().ended.unwrap_or_default();
    assert!(ended.contains("erwartet war"), "{ended}");
}

#[tokio::test]
async fn an_existing_branch_of_somebody_else_stops_the_run_before_the_container_starts() {
    let mut w = welt(true);
    w.state = with_repos(
        &w,
        with_pattern("feature/{ticket}-{slug}"),
        Some("firma/web"),
    );
    let remote = w.tmp.path().join("git/firma/web.git");
    let g = std::process::Command::new("git")
        .current_dir(&remote)
        .args(["branch", "feature/BAU-142-toggle-fokus-ring-fehlt", "main"])
        .output()
        .unwrap();
    assert!(g.status.success());
    w.write(
        "scenario-run.sh",
        &scenario_on("feature/BAU-142-toggle-fokus-ring-fehlt"),
    );
    start(&w, AgentOptions::default()).await.unwrap();
    w.wait("Lauf endet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    w.wait_for_end().await;
    let ended = w.run().ended.unwrap_or_default();
    assert!(ended.contains("gibt es im Repo schon"), "{ended}");
}
