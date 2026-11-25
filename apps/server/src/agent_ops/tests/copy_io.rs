//! Strategy "copy" (Cockpit runs in a container, no bind mounts): the fake `docker` keeps a folder
//! per container (`ctr-<name>/work` and `/job`) and implements `create`, `cp` and `start`.

use super::super::copy_io::{open_up, tighten};
use super::followup::pr_world;
use super::*;
use cockpit_i18n::Lang;
use wiremock::MockServer;

/// The same world, but with `COCKPIT_AGENT_IO=copy`.
fn copy_mode(mut w: World) -> World {
    let settings = Settings {
        agent_io: Some("copy".into()),
        ..w.state.inner.settings.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("copy.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        settings,
    );
    state.inner.live.set_state_for_test(w.ws());
    let repos = w.state.inner.agent_repo_config.read().unwrap().clone();
    *state.inner.agent_repo_config.write().unwrap() = repos;
    let sources = w.state.inner.sources.read().unwrap().clone();
    *state.inner.sources.write().unwrap() = sources;
    w.state = state;
    w
}

/// Names of the container folders the fake still has (a container that was not removed).
fn containers_left(w: &World) -> Vec<String> {
    std::fs::read_dir(w.bin())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("ctr-"))
        .collect()
}

fn calls(w: &World) -> Vec<String> {
    w.log("calls.log").lines().map(str::to_string).collect()
}

#[tokio::test]
async fn a_run_copies_in_starts_copies_out_and_pushes_from_the_backend() {
    let w = copy_mode(world(true));
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    let run = w.run();
    assert_eq!(run.files, ["neu.txt"]);
    assert!(
        german(&run.summary).contains("gepusht"),
        "{:?}",
        run.summary
    );
    assert_eq!(w.branches(), [BRANCH, "main"]);

    let all = calls(&w);
    assert!(!all.iter().any(|z| z.starts_with("run ")), "{all:?}");
    let create = all
        .iter()
        .find(|z| z.starts_with("create "))
        .expect("create was called");
    for expected in [
        "-i",
        "--label cockpit.run=BAU-142",
        "--add-host cockpit-host:host-gateway",
        "--cap-drop ALL",
        "-e ANTHROPIC_AUTH_TOKEN",
        "localhost/cockpit-agent:local",
    ] {
        assert!(create.contains(expected), "{expected} is missing: {create}");
    }
    assert!(
        !create.contains(" -v ") && !create.contains("--rm"),
        "no mounts and no --rm: {create}"
    );
    let name = create
        .split_whitespace()
        .skip_while(|a| *a != "--name")
        .nth(1)
        .unwrap()
        .to_string();
    // Order: create, copy clone and job in, start attached, copy out, remove.
    let pos = |what: &dyn Fn(&str) -> bool| all.iter().position(|z| what(z)).unwrap();
    let c = pos(&|z| z.starts_with("create "));
    let cp_work = pos(&|z| z.starts_with("cp ") && z.ends_with(&format!("{name}:/work")));
    let cp_job = pos(&|z| z.starts_with("cp ") && z.ends_with(&format!("{name}:/job")));
    let st = pos(&|z| z == format!("start -a -i {name}"));
    let out = pos(&|z| z.starts_with(&format!("cp {name}:/work/. ")));
    let rm = pos(&|z| z == format!("rm -f {name}"));
    assert!(
        c < cp_work && c < cp_job && cp_work < st && cp_job < st && st < out && out < rm,
        "{all:?}"
    );
    // No secret in any argument; the value only arrives through the environment of `create`.
    let text = w.log("calls.log");
    assert!(!text.contains(TOKEN) && !text.contains(GITHUB));
    let env = w.log("env.log");
    assert!(
        env.contains(&format!("TOKEN={TOKEN}")) && env.contains("MODEL=claude-sonnet-5-5"),
        "{env}"
    );
    // Cleanup: no container left, no folder left.
    assert!(containers_left(&w).is_empty());
    assert!(!w.state.agent_dir().join("runs").join("BAU-142").exists());
}

#[tokio::test]
async fn plan_and_implementation_both_copy_and_the_plan_is_never_copied_out() {
    let w = copy_mode(world(true));
    w.write(
        "scenario-plan.sh",
        r#"printf '%s\n' '{"t":"plan","summary":"Fokus-Ring ergänzen","files":["neu.txt"],"steps":["Lesen"]}'
exit 0
"#,
    );
    w.write("scenario-run.sh", &scenario_ok());
    w.start(true).await;
    w.wait("Plan liegt vor", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.agent_plan.is_some())
    })
    .await;
    w.wait_for_end().await;
    assert!(
        !calls(&w).iter().any(|z| z.starts_with("cp cockpit-run")),
        "a plan changes nothing, so nothing is copied out"
    );
    assert!(containers_left(&w).is_empty());
    assert_eq!(w.branches(), ["main"]);
    w.action(Action::ApprovePlan {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches(), [BRANCH, "main"]);
    assert_eq!(
        calls(&w)
            .iter()
            .filter(|z| z.starts_with("create "))
            .count(),
        2
    );
    assert!(containers_left(&w).is_empty());
}

#[tokio::test]
async fn a_halted_run_copies_its_work_out_and_keeps_the_clone() {
    let w = copy_mode(world(true));
    w.write(
        "scenario-run.sh",
        r#"echo halb > "$clone/halb.txt"
printf '%s\n' '{"t":"step","title":"Viel gelesen","text":"","tokens_in":1000000,"tokens_out":0,"ms":10}'
while [ ! -e "$d/stopped-$name" ]; do sleep 0.05; done
exit 137
"#,
    );
    let mut limits = w.ws().limits.clone();
    limits.run_budget_cents = Some(50);
    w.action(Action::SetAgentLimits { limits }).await.unwrap();
    w.start(false).await;
    w.wait("angehalten", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_stopped())
    })
    .await;
    w.wait_for_end().await;
    let all = calls(&w);
    let out = all
        .iter()
        .position(|z| z.starts_with("cp cockpit-run-BAU-142-"))
        .expect("the work is copied out before the container goes");
    let rm = all
        .iter()
        .position(|z| z.starts_with("rm -f cockpit-run-BAU-142-"))
        .unwrap();
    assert!(out < rm, "{all:?}");
    let clone = w.state.agent_dir().join("runs/BAU-142/run");
    assert!(clone.join(".git").exists());
    assert!(clone.join("halb.txt").exists(), "the work so far is kept");
    assert!(containers_left(&w).is_empty());
    assert_eq!(w.branches(), ["main"]);
}

#[tokio::test]
async fn cancelling_removes_the_container_and_the_folder() {
    let w = copy_mode(world(true));
    w.write(
        "scenario-run.sh",
        r#"while [ ! -e "$d/stopped-$name" ]; do sleep 0.05; done
exit 137
"#,
    );
    w.start(false).await;
    w.wait("Container läuft", |_| {
        !lock(&w.state.inner.agent.running).is_empty() && w.log("calls.log").contains("start ")
    })
    .await;
    w.action(Action::CancelAgent {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait_for_end().await;
    assert!(
        calls(&w)
            .iter()
            .any(|z| z.starts_with("rm -f cockpit-run-BAU-142-"))
    );
    assert!(containers_left(&w).is_empty());
    assert!(!w.state.agent_dir().join("runs/BAU-142").exists());
    assert_eq!(w.branches(), ["main"]);
}

#[tokio::test]
async fn a_failing_create_is_reported_and_leaves_nothing() {
    let w = copy_mode(world(true));
    w.write("create-error", "");
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("Fehler", |_| {
        lock(&w.state.inner.agent.failed).contains_key("BAU-142")
    })
    .await;
    w.wait_for_end().await;
    let run = w.run();
    assert!(
        run.messages
            .iter()
            .any(|m| m.text_in(Lang::De).contains("no such image")),
        "{:?}",
        run.messages
    );
    assert!(!calls(&w).iter().any(|z| z.starts_with("start ")));
    assert!(containers_left(&w).is_empty());
    assert_eq!(w.branches(), ["main"]);
}

#[tokio::test]
async fn a_failing_copy_in_removes_the_container_again() {
    let w = copy_mode(world(true));
    w.write("cp-error", "");
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("Fehler", |_| {
        lock(&w.state.inner.agent.failed).contains_key("BAU-142")
    })
    .await;
    w.wait_for_end().await;
    let run = w.run();
    assert!(
        run.messages
            .iter()
            .any(|m| m.text_in(Lang::De).contains("kopieren")
                && m.text_in(Lang::De).contains("No such container")),
        "{:?}",
        run.messages
    );
    let all = calls(&w);
    assert!(!all.iter().any(|z| z.starts_with("start ")), "{all:?}");
    assert!(all.iter().any(|z| z.starts_with("rm -f cockpit-run-")));
    assert!(containers_left(&w).is_empty());
}

// ---------- several repos ----------

fn two_repos(github: &MockServer) -> World {
    let w = pr_world(github);
    *w.state.inner.agent_repo_config.write().unwrap() = crate::app::RepoConfig {
        repos: vec![
            cockpit_core::AgentRepo::new("firma/app"),
            cockpit_core::AgentRepo::new("firma/web"),
        ],
        default: Some("firma/app".into()),
    };
    let mut ws = demo::workspace();
    ws.set_repo_rules(
        "firma/web",
        cockpit_core::RepoRules {
            forbidden_paths: vec!["secrets/".into()],
            ..Default::default()
        },
    );
    w.state.inner.live.set_state_for_test(ws);
    copy_mode(w)
}

fn scenario_two(web_part: &str) -> String {
    format!(
        r#"echo neu > "$clone/firma__app/neu.txt"
git -C "$clone/firma__app" add -A
git -C "$clone/firma__app" -c user.name=a -c user.email=a@a.invalid commit -qm Agent
{web_part}
printf '%s\n' \
 '{{"t":"result","text":"In zwei Repos geändert.","branch":"{BRANCH}","commit":"abc1234","repos":[{{"repo":"firma/app","branch":"{BRANCH}","commit":"abc1234"}}]}}'
exit 0
"#
    )
}

async fn run_two(w: &World) {
    w.action(Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            plan_first: false,
            repos: vec!["firma/app".into(), "firma/web".into()],
            ..AgentOptions::default()
        },
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
}

#[tokio::test]
async fn several_repos_travel_in_one_copy_and_each_is_checked_and_pushed_by_the_backend() {
    let github = MockServer::start().await;
    let w = two_repos(&github);
    // `web` breaks its path rule, `app` is fine: the same outcome as with bind mounts.
    w.write(
        "scenario-run.sh",
        &scenario_two(
            r#"mkdir -p "$clone/firma__web/secrets"
echo geheim > "$clone/firma__web/secrets/key.txt"
git -C "$clone/firma__web" add -A
git -C "$clone/firma__web" -c user.name=a -c user.email=a@a.invalid commit -qm Agent"#,
        ),
    );
    run_two(&w).await;
    assert_eq!(
        w.branches_of("app"),
        ["claude/BAU-127-list-separator", BRANCH, "main"]
    );
    assert_eq!(
        w.branches_of("web"),
        ["main"],
        "the refused repo got nothing"
    );
    let run = w.run();
    assert_eq!(run.repos[0].state, cockpit_core::RunRepoState::Pushed);
    assert_eq!(run.repos[1].state, cockpit_core::RunRepoState::Refused);
    // One copy in and one out for the whole run folder (`/work` holds both repos).
    let all = calls(&w);
    assert_eq!(
        all.iter()
            .filter(|z| z.starts_with("cp ") && z.contains(":/work"))
            .count(),
        2,
        "{all:?}"
    );
    assert!(all.iter().any(|z| z.contains("/run/. cockpit-run-")));
    assert!(containers_left(&w).is_empty());
    assert!(github.received_requests().await.unwrap().is_empty());
}

// ---------- the reachability check ----------

#[tokio::test]
async fn the_check_container_also_runs_without_mounts() {
    let w = copy_mode(world(true));
    w.write(
        "scenario-plan.sh",
        "printf '%s\\n' '{\"t\":\"check\",\"reachable\":true,\"key_ok\":true,\"hint\":\"ok\"}'\nexit 0\n",
    );
    let p = w.state.container_check().await;
    assert!(p.checked && p.container_reached, "{p:?}");
    let all = calls(&w);
    assert!(
        all.iter()
            .any(|z| z.starts_with("create ") && !z.contains(" -v "))
    );
    assert!(
        all.iter()
            .any(|z| z.starts_with("start -a cockpit-run-check-"))
    );
    assert!(
        all.iter()
            .any(|z| z.starts_with("rm -f cockpit-run-check-"))
    );
    assert!(containers_left(&w).is_empty());
}

// ---------- the choice ----------

#[test]
fn the_strategy_follows_the_environment_and_otherwise_the_place_cockpit_runs_in() {
    use agent_run::AgentIo::{Bind, Copy};
    assert_eq!(agent_run::AgentIo::choose(None, false), Bind);
    assert_eq!(agent_run::AgentIo::choose(None, true), Copy);
    assert_eq!(agent_run::AgentIo::choose(Some("bind"), true), Bind);
    assert_eq!(agent_run::AgentIo::choose(Some(" COPY "), false), Copy);
    assert_eq!(agent_run::AgentIo::choose(Some(""), true), Copy);
}

#[cfg(unix)]
#[test]
fn open_up_and_tighten_set_the_modes_without_following_symlinks() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("c");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/a.txt"), "a").unwrap();
    std::fs::write(d.join("run.sh"), "#!/bin/sh").unwrap();
    std::fs::set_permissions(d.join("sub/a.txt"), std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(d.join("run.sh"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", d.join("link")).unwrap();
    let mode = |p: &str| std::fs::metadata(d.join(p)).unwrap().permissions().mode() & 0o777;
    open_up(&d).unwrap();
    assert_eq!(mode("sub/a.txt"), 0o666, "files: rw for all, no x");
    assert_eq!(mode("run.sh"), 0o777, "executable stays executable for all");
    assert_eq!(mode("sub"), 0o777);
    tighten(&d).unwrap();
    assert_eq!(mode("sub/a.txt"), 0o644);
    assert_eq!(mode("run.sh"), 0o755);
    assert_eq!(mode("sub"), 0o755);
}
