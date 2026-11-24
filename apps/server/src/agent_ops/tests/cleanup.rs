//! „Aufräumen“ with the fake `docker` and temp folders: preview first, only confirmed orphans go,
//! never the image and never a container of someone else.

use super::*;
use cockpit_core::api::CleanupRequest;
use cockpit_core::{AgentRun, RunState};

/// BAU-142 works (a container of its own), nothing else does.
fn busy_world() -> World {
    let w = welt(true);
    let mut run = AgentRun::new("BAU-142", "Toggle");
    run.state = RunState::Running;
    let mut ws = demo::workspace();
    ws.runs = vec![run];
    w.state.inner.live.set_state_for_test(ws);
    w
}

fn runs_dir(w: &World) -> PathBuf {
    w.state.agent_dir().join("runs")
}

fn make_folders(w: &World, names: &[&str]) {
    for n in names {
        let d = runs_dir(w).join(n).join("run");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("f.txt"), "x").unwrap();
    }
}

async fn preview(w: &World) -> cockpit_core::api::CleanupPreview {
    let s = w.state.clone();
    tokio::task::spawn_blocking(move || s.cleanup_preview())
        .await
        .unwrap()
}

async fn apply(w: &World, request: CleanupRequest) -> cockpit_core::api::CleanupResult {
    let s = w.state.clone();
    tokio::task::spawn_blocking(move || s.cleanup_apply(&request))
        .await
        .unwrap()
}

#[tokio::test]
async fn the_preview_lists_orphans_and_removes_nothing() {
    let w = busy_world();
    w.write(
        "ps.out",
        "abc123 cockpit-run-BAU-142-1700\ndef456 cockpit-run-BAU-7-1600\nfff000 cockpit-run-old-1\n",
    );
    make_folders(&w, &["BAU-142", "BAU-7", "alt"]);
    let p = preview(&w).await;
    let ids: Vec<&str> = p.containers.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        ["def456", "fff000"],
        "the container of the working run stays"
    );
    assert_eq!(p.folders, ["BAU-7", "alt"]);
    assert!(!p.engine_missing && !p.is_empty());
    let calls = w.log("calls.log");
    assert!(
        calls.contains("ps -a --filter label=cockpit.run --format {{.ID}} {{.Names}}"),
        "{calls}"
    );
    assert!(!calls.contains("rm "), "a preview removes nothing: {calls}");
    assert!(runs_dir(&w).join("BAU-7").exists());
}

#[tokio::test]
async fn only_the_confirmed_orphans_are_removed() {
    let w = busy_world();
    w.write(
        "ps.out",
        "abc123 cockpit-run-BAU-142-1700\ndef456 cockpit-run-BAU-7-1600\nfff000 cockpit-run-old-1\n",
    );
    make_folders(&w, &["BAU-142", "BAU-7", "alt"]);
    let r = apply(
        &w,
        CleanupRequest {
            // `abc123` belongs to a working run, `zzz999` is unknown: neither is touched.
            containers: vec!["def456".into(), "abc123".into(), "zzz999".into()],
            folders: vec!["BAU-7".into(), "BAU-142".into(), "..".into()],
        },
    )
    .await;
    assert_eq!((r.containers, r.folders), (1, 1));
    let calls = w.log("calls.log");
    assert!(calls.contains("rm -f def456\n"), "{calls}");
    let removals: Vec<&str> = calls.lines().filter(|l| l.starts_with("rm ")).collect();
    assert_eq!(
        removals,
        ["rm -f def456"],
        "only the confirmed orphan: {calls}"
    );
    assert!(
        !calls.contains("rmi") && !calls.contains("image rm"),
        "{calls}"
    );
    assert!(!runs_dir(&w).join("BAU-7").exists());
    assert!(
        runs_dir(&w).join("BAU-142").exists(),
        "the working run keeps its clone"
    );
    assert!(runs_dir(&w).join("alt").exists(), "unconfirmed stays");
}

#[tokio::test]
async fn a_finished_run_leaves_its_clone_as_an_orphan() {
    let w = welt(true);
    let mut run = AgentRun::new("BAU-142", "Toggle");
    run.state = RunState::Done;
    let mut ws = demo::workspace();
    ws.runs = vec![run];
    w.state.inner.live.set_state_for_test(ws);
    make_folders(&w, &["BAU-142"]);
    assert_eq!(preview(&w).await.folders, ["BAU-142"]);
}

#[tokio::test]
async fn a_halted_run_keeps_its_clone() {
    let w = welt(true);
    let mut run = AgentRun::new("BAU-142", "Toggle");
    run.state = RunState::Stopped(cockpit_core::StopReason::Time);
    let mut ws = demo::workspace();
    ws.runs = vec![run];
    w.state.inner.live.set_state_for_test(ws);
    make_folders(&w, &["BAU-142"]);
    assert!(preview(&w).await.folders.is_empty());
}

#[tokio::test]
async fn without_a_container_program_only_folders_are_listed() {
    let w = busy_world();
    w.write("no-engine", "");
    make_folders(&w, &["alt"]);
    let p = preview(&w).await;
    assert!(p.engine_missing);
    assert!(p.containers.is_empty());
    assert_eq!(p.folders, ["alt"]);
    let r = apply(
        &w,
        CleanupRequest {
            containers: vec!["abc".into()],
            folders: vec!["alt".into()],
        },
    )
    .await;
    assert_eq!((r.containers, r.folders), (0, 1));
    assert!(!w.log("calls.log").contains("rm -f"));
}

#[tokio::test]
async fn nothing_to_clean_is_an_empty_preview() {
    let w = busy_world();
    let p = preview(&w).await;
    assert!(p.is_empty(), "{p:?}");
}

/// Removing runs from the list cleans up only what those runs left: their labelled containers and
/// folders. A working run, other orphans and unlabelled containers stay.
#[tokio::test]
async fn removing_runs_cleans_up_only_their_leftovers() {
    let w = welt(true);
    let mut ws = demo::workspace();
    let mut done = AgentRun::new("BAU-7", "Done");
    done.state = RunState::Done;
    let mut busy = AgentRun::new("BAU-142", "Busy");
    busy.state = RunState::Running;
    ws.runs = vec![done, busy];
    w.state.inner.live.set_state_for_test(ws);
    w.write(
        "ps.out",
        "aaa111 cockpit-run-BAU-7-1600\nbbb222 cockpit-run-BAU-142-1700\nccc333 cockpit-run-old-1\n",
    );
    make_folders(&w, &["BAU-7", "BAU-142", "alt"]);

    w.state
        .action_run(cockpit_core::Action::RemoveFinishedRuns)
        .await
        .unwrap();

    let calls = w.log("calls.log");
    let removals: Vec<&str> = calls.lines().filter(|l| l.starts_with("rm ")).collect();
    assert_eq!(removals, ["rm -f aaa111"], "{calls}");
    assert!(!runs_dir(&w).join("BAU-7").exists());
    assert!(runs_dir(&w).join("BAU-142").exists(), "the working run");
    assert!(runs_dir(&w).join("alt").exists(), "other orphans stay");
    let keys: Vec<String> = w
        .state
        .inner
        .live
        .copy()
        .runs
        .iter()
        .map(|r| r.key())
        .collect();
    assert_eq!(keys, ["BAU-142"]);
}

#[tokio::test]
async fn a_working_run_is_not_removed_and_nothing_is_cleaned() {
    let w = busy_world();
    w.write("ps.out", "bbb222 cockpit-run-BAU-142-1700\n");
    make_folders(&w, &["BAU-142"]);
    let err = w
        .state
        .action_run(cockpit_core::Action::RemoveAgentRun {
            run: "BAU-142".into(),
        })
        .await;
    assert!(err.is_err());
    assert!(!w.log("calls.log").contains("rm "));
    assert!(runs_dir(&w).join("BAU-142").exists());
}
