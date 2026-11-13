use super::*;
use crate::app::Settings;
use crate::config::{AgentChoice, EndpointConfig};
use crate::secrets;
use crate::secrets::{MemoryStore, SecretStore};
use cockpit_core::api::Provider;
use cockpit_core::{AgentOptions, MessageKind, demo};

const TOKEN: &str = "sk-test-geheim-123";
const GITHUB: &str = "ghp_test_geheim_456";

// ---------- mit Attrappe als docker und lokalem Git-Remote ----------

/// `docker` als Shell-Skript. Das Szenario eines Laufs steht in `scenario-plan.sh` bzw.
/// `scenario-run.sh` (je nachdem, ob der Klon schreibgeschützt eingebunden ist) und wird
/// im Skript eingelesen; `$clone`, `$name` und `$d` stehen darin zur Verfügung.
const DOCKER: &str = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$@" >> "$d/calls.log"
case "$1" in
  info) exit 0 ;;
  image) [ -e "$d/no-image" ] && exit 1; exit 0 ;;
  build) cat "$d/build.out" 2>/dev/null; [ -e "$d/build.error" ] && { echo "write: no space left on device" >&2; exit 1; }; exit 0 ;;
  ps) cat "$d/ps.out" 2>/dev/null; exit 0 ;;
  rm) for a in "$@"; do touch "$d/stopped-$a"; done; exit 0 ;;
  pause|unpause) echo "$1" >> "$d/pause.log"; exit 0 ;;
  run)
shift
name=""; clone=""; mode="run"; prev=""
for a in "$@"; do
  [ "$prev" = "--name" ] && name="$a"
  case "$a" in
    *:/work:ro) clone="${a%%:/work:ro}"; mode="plan" ;;
    *:/work) clone="${a%%:/work}" ;;
  esac
  prev="$a"
done
echo "TOKEN=$ANTHROPIC_AUTH_TOKEN BASE=$ANTHROPIC_BASE_URL MODEL=$ANTHROPIC_MODEL" >> "$d/env.log"
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
        let out = std::process::Command::new("git")
            .current_dir(self.remote())
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
                crate::sync::ActionFailure::Plan(m) => m,
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
    /// Lässt den Aufpasser arbeiten, bis `bedingung` gilt (höchstens 15 Sekunden).
    async fn wait(&self, was: &str, condition: impl Fn(&Workspace) -> bool) {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            self.state.watch().await;
            if condition(&self.ws()) {
                return;
            }
            assert!(
                Instant::now() < end,
                "Zeitüberschreitung beim Warten auf: {was}\n{:#?}",
                self.run()
            );
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    async fn wait_for_end(&self) {
        let end = Instant::now() + Duration::from_secs(15);
        while !lock(&self.state.inner.agent.running).is_empty() {
            self.state.watch().await;
            assert!(Instant::now() < end, "der Lauf endet nicht");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
}

/// Eine World mit allem eingerichtet. `echt = false` lässt den Container-Teil weg.
fn welt(set_up: bool) -> World {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let docker = bin.join("docker");
    std::fs::write(&docker, DOCKER).unwrap();
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Ein Remote mit `main`.
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

    let agent_dir = tmp.path().join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(agent_dir.join("Containerfile"), "FROM scratch\n").unwrap();

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
            provider: Provider::ClaudeAgentSdk,
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

/// Szenario: Der „Container“ ändert eine Datei im Klon, committet und meldet das Ergebnis.
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

/// Ein Zustand wie `welt`, aber mit anderer Wahl von Anbieter und Endpunkt.
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

mod container_check;
mod maintenance;
mod pure;
mod run;
mod start;
