use super::*;
use crate::app::Settings;
use crate::config::{AgentChoice, EndpointConfig};
use crate::secrets;
use crate::secrets::{MemoryStore, SecretStore};
use cockpit_core::api::Provider;
use cockpit_core::{AgentOptions, MessageKind, demo};

const TOKEN: &str = "sk-test-geheim-123";
const GITHUB: &str = "ghp_test_geheim_456";

// ---------- with a fake as docker and a local Git remote ----------

/// `docker` as a shell script. The scenario of a run is in `scenario-plan.sh` or
/// `scenario-run.sh` (depending on whether the clone is mounted read-only) and is read in by
/// the script; `$clone`, `$name` and `$d` are available in it.
const DOCKER: &str = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$@" >> "$d/calls.log"
case "$1" in
  info) [ -e "$d/no-engine" ] && exit 1; exit 0 ;;
  image) [ -e "$d/no-image" ] && exit 1; [ -e "$d/inspect-error" ] && { echo "boom" >&2; exit 125; }; cat "$d/label.out" 2>/dev/null; exit 0 ;;
  build) cat "$d/build.out" 2>/dev/null; [ -e "$d/build.error" ] && { echo "write: no space left on device" >&2; exit 1; }; exit 0 ;;
  ps) cat "$d/ps.out" 2>/dev/null; exit 0 ;;
  rm) for a in "$@"; do touch "$d/stopped-$a"; rm -rf "$d/ctr-$a"; done; exit 0 ;;
  create)
shift
name=""; prev=""
for a in "$@"; do
  [ "$prev" = "--name" ] && name="$a"
  prev="$a"
done
[ -e "$d/create-error" ] && { echo "Error response from daemon: no such image" >&2; exit 125; }
mkdir -p "$d/ctr-$name/work" "$d/ctr-$name/job"
echo "TOKEN=$ANTHROPIC_AUTH_TOKEN BASE=$ANTHROPIC_BASE_URL MODEL=$ANTHROPIC_MODEL PROVIDER=$COCKPIT_PROVIDER OAIKEY=$OPENAI_API_KEY OAIBASE=$OPENAI_BASE_URL CMODEL=$COCKPIT_MODEL ANTKEY=$ANTHROPIC_API_KEY" >> "$d/env.log"
exit 0 ;;
  cp)
[ -e "$d/cp-error" ] && { echo "Error: No such container" >&2; exit 1; }
src="$2"; dst="$3"
case "$dst" in
  *:*) n="${dst%%:*}"; p="${dst#*:}"; [ -d "$d/ctr-$n" ] || exit 1
       mkdir -p "$d/ctr-$n$p"; cp -a "${src%/.}/." "$d/ctr-$n$p/" || exit 1 ;;
  *) n="${src%%:*}"; p="${src#*:}"; p="${p%/.}"; [ -d "$d/ctr-$n$p" ] || exit 1
     mkdir -p "$dst"; cp -a "$d/ctr-$n$p/." "$dst/" || exit 1 ;;
esac
exit 0 ;;
  start)
for a in "$@"; do name="$a"; done
clone="$d/ctr-$name/work"; job="$d/ctr-$name/job"
[ -d "$d/ctr-$name" ] || exit 125
mode="run"
grep -q '"mode":"\(plan\|review\|check\)"' "$job/job.json" && mode="plan"
. "$d/scenario-$mode.sh"
;;
  pause|unpause) echo "$1" >> "$d/pause.log"; exit 0 ;;
  run)
shift
name=""; clone=""; mode="run"; prev=""
for a in "$@"; do
  [ "$prev" = "--name" ] && name="$a"
  case "$a" in
    *:/work:ro) clone="${a%%:/work:ro}"; mode="plan" ;;
    *:/work) clone="${a%%:/work}" ;;
    *:/job:ro) job="${a%%:/job:ro}" ;;
  esac
  prev="$a"
done
echo "TOKEN=$ANTHROPIC_AUTH_TOKEN BASE=$ANTHROPIC_BASE_URL MODEL=$ANTHROPIC_MODEL PROVIDER=$COCKPIT_PROVIDER OAIKEY=$OPENAI_API_KEY OAIBASE=$OPENAI_BASE_URL CMODEL=$COCKPIT_MODEL ANTKEY=$ANTHROPIC_API_KEY" >> "$d/env.log"
. "$d/scenario-$mode.sh"
;;
esac
exit 0
"#;

struct World {
    tmp: tempfile::TempDir,
    state: AppState,
    secrets: Arc<MemoryStore>,
}

fn sh(cwd: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@t.invalid")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@t.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

impl World {
    fn bin(&self) -> PathBuf {
        self.tmp.path().join("bin")
    }
    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.bin().join(name), content).unwrap();
    }
    fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.bin().join(name)).unwrap_or_default()
    }
    fn remote(&self) -> PathBuf {
        self.tmp.path().join("git/firma/app.git")
    }
    fn branches(&self) -> Vec<String> {
        self.branches_of("app")
    }
    /// Branches of the bare remote `firma/<name>`.
    fn branches_of(&self, name: &str) -> Vec<String> {
        let out = std::process::Command::new("git")
            .current_dir(self.tmp.path().join(format!("git/firma/{name}.git")))
            .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
            .output()
            .unwrap();
        let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        v.sort();
        v
    }
    fn ws(&self) -> Workspace {
        self.state.inner.live.copy()
    }
    fn run(&self) -> cockpit_core::AgentRun {
        self.ws().run("BAU-142").unwrap()
    }
    async fn action(&self, a: Action) -> Result<(), String> {
        self.state
            .action_run(a)
            .await
            .map(|_| ())
            .map_err(|e| match e {
                crate::sync::ActionFailure::Plan(m) => m.de(),
                crate::sync::ActionFailure::Service(m) => m.de(),
                crate::sync::ActionFailure::Rule(e) => e.message(),
                other => format!("{other:?}"),
            })
    }
    async fn start(&self, plan_first: bool) {
        self.action(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions {
                plan_first,
                ..AgentOptions::default()
            },
        })
        .await
        .unwrap();
    }
    /// Lets the watcher work until `condition` holds (at most 15 seconds).
    async fn wait(&self, was: &str, condition: impl Fn(&Workspace) -> bool) {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            self.state.watch().await;
            if condition(&self.ws()) {
                return;
            }
            assert!(
                Instant::now() < end,
                "Timed out waiting for: {was}\n{:#?}",
                self.run()
            );
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    async fn wait_for_end(&self) {
        let end = Instant::now() + Duration::from_secs(15);
        while !lock(&self.state.inner.agent.running).is_empty() {
            self.state.watch().await;
            assert!(Instant::now() < end, "the run does not end");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
}

/// A world with everything set up. `real = false` leaves out the container part.
fn welt(set_up: bool) -> World {
    welt_with(set_up, Provider::ClaudeAgentSdk)
}

/// Like [`welt`], with the tool of the agent chosen.
fn welt_with(set_up: bool, provider: Provider) -> World {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let docker = bin.join("docker");
    std::fs::write(&docker, DOCKER).unwrap();
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();

    // A remote with `main`.
    let source = tmp.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    sh(&source, &["init", "--quiet", "-b", "main"]);
    std::fs::write(source.join("README.md"), "hallo\n").unwrap();
    sh(&source, &["add", "-A"]);
    sh(&source, &["commit", "--quiet", "-m", "Anfang"]);
    std::fs::create_dir_all(tmp.path().join("git/firma")).unwrap();
    sh(
        tmp.path(),
        &["clone", "--quiet", "--bare", "source", "git/firma/app.git"],
    );
    // A second repo of the same shape, for the tests with several repos.
    sh(
        tmp.path(),
        &["clone", "--quiet", "--bare", "source", "git/firma/web.git"],
    );

    let agent_dir = tmp.path().join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    for (p, c) in [
        ("Containerfile", "FROM scratch\n"),
        ("runner/package.json", "{}\n"),
        ("runner/package-lock.json", "{}\n"),
        ("runner/src/main.ts", "run\n"),
        ("clis/package.json", "{}\n"),
        ("clis/package-lock.json", "{}\n"),
    ] {
        let f = agent_dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    // The fake image carries the current fingerprint, unless a test says otherwise.
    std::fs::write(
        bin.join("label.out"),
        format!("{}\n", crate::agent_image::fingerprint(&agent_dir).unwrap()),
    )
    .unwrap();

    let secrets = Arc::new(MemoryStore::new());
    if set_up {
        secrets.set(secrets::LITELLM, &Secret::new(TOKEN)).unwrap();
        secrets.set(secrets::GITHUB, &Secret::new(GITHUB)).unwrap();
    }
    let settings = Settings {
        seed: Seed::Empty,
        container_env: Some("docker".into()),
        search_path: Some(bin.clone().into_os_string()),
        agent: AgentChoice {
            provider,
            endpoint: set_up.then(|| EndpointConfig {
                url: Some("https://litellm.example".into()),
                model: Some("claude-sonnet-5-5".into()),
            }),
        },
        agent_repo: Some("firma/app".into()),
        agent_dir: Some(agent_dir),
        git_base: Some(format!("file://{}/git", tmp.path().display())),
        data_dir: Some(tmp.path().join("daten")),
        ..Settings::default()
    };
    let conn = crate::db::open(&tmp.path().join("t.db")).unwrap();
    let state = AppState::create(
        conn,
        secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        settings,
    );
    state.inner.live.set_state_for_test(demo::workspace());
    World {
        tmp,
        state,
        secrets,
    }
}

const BRANCH: &str = "claude/BAU-142-toggle-fokus-ring-fehlt";

/// Scenario: the "container" changes a file in the clone, commits and reports the result.
fn scenario_ok() -> String {
    format!(
        r#"echo neu > "$clone/neu.txt"
git -C "$clone" add -A
git -C "$clone" -c user.name=a -c user.email=a@a.invalid commit -qm Agent
printf '%s\n' \
 '{{"t":"step","title":"Datei ändern","text":"neu.txt","tokens_in":1000,"tokens_out":200,"ms":1500}}' \
 '{{"t":"files","files":["neu.txt"]}}' \
 '{{"t":"result","text":"Fokus-Ring ergänzt.","branch":"{BRANCH}","commit":"abc1234"}}'
exit 0
"#
    )
}

/// A state like `world`, but with these repos for the agent (and this default).
fn with_repos(w: &World, repos: Vec<cockpit_core::AgentRepo>, default: Option<&str>) -> AppState {
    let e = Settings {
        agent_repos: repos,
        agent_repo: default.map(str::to_string),
        ..w.state.inner.settings.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("repos.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        e,
    );
    state.inner.live.set_state_for_test(demo::workspace());
    state
}

/// A state like `world`, but with a different choice of provider and endpoint.
fn with_choice(w: &World, choice: AgentChoice) -> AppState {
    let e = Settings {
        agent: choice,
        ..w.state.inner.settings.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("choice.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        e,
    );
    state.inner.live.set_state_for_test(demo::workspace());
    state
}

mod agent_review;
mod branch_delete;
mod branch_pattern;
mod cleanup;
mod container_check;
mod copy_io;
mod followup;
mod maintenance;
mod pure;
mod run;
mod start;
mod suggest;
