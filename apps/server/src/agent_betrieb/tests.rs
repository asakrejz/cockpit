use super::*;
use crate::app::Einstellungen;
use crate::config::{AgentWahl, EndpunktKonfig};
use crate::secrets;
use crate::secrets::{MemoryStore, SecretStore};
use cockpit_core::api::Provider;
use cockpit_core::{AgentOptions, MessageKind, demo};

const TOKEN: &str = "sk-test-geheim-123";
const GITHUB: &str = "ghp_test_geheim_456";

// ---------- mit Attrappe als docker und lokalem Git-Remote ----------

/// `docker` als Shell-Skript. Das Szenario eines Laufs steht in `szenario-plan.sh` bzw.
/// `szenario-lauf.sh` (je nachdem, ob der Klon schreibgeschützt eingebunden ist) und wird
/// im Skript eingelesen; `$klon`, `$name` und `$d` stehen darin zur Verfügung.
const DOCKER: &str = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$@" >> "$d/aufrufe.log"
case "$1" in
  info) exit 0 ;;
  image) [ -e "$d/kein-image" ] && exit 1; exit 0 ;;
  build) cat "$d/build.out" 2>/dev/null; [ -e "$d/build.fehler" ] && { echo "write: no space left on device" >&2; exit 1; }; exit 0 ;;
  ps) cat "$d/ps.out" 2>/dev/null; exit 0 ;;
  rm) for a in "$@"; do touch "$d/gestoppt-$a"; done; exit 0 ;;
  pause|unpause) echo "$1" >> "$d/pause.log"; exit 0 ;;
  run)
shift
name=""; klon=""; modus="lauf"; prev=""
for a in "$@"; do
  [ "$prev" = "--name" ] && name="$a"
  case "$a" in
    *:/work:ro) klon="${a%%:/work:ro}"; modus="plan" ;;
    *:/work) klon="${a%%:/work}" ;;
  esac
  prev="$a"
done
echo "TOKEN=$ANTHROPIC_AUTH_TOKEN BASE=$ANTHROPIC_BASE_URL MODEL=$ANTHROPIC_MODEL" >> "$d/umgebung.log"
. "$d/szenario-$modus.sh"
;;
esac
exit 0
"#;

struct Welt {
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

impl Welt {
    fn bin(&self) -> PathBuf {
        self.tmp.path().join("bin")
    }
    fn schreibe(&self, name: &str, inhalt: &str) {
        std::fs::write(self.bin().join(name), inhalt).unwrap();
    }
    fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.bin().join(name)).unwrap_or_default()
    }
    fn remote(&self) -> PathBuf {
        self.tmp.path().join("git/firma/app.git")
    }
    fn zweige(&self) -> Vec<String> {
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
        self.state.inner.live.kopie()
    }
    fn lauf(&self) -> cockpit_core::AgentRun {
        self.ws().run("BAU-142").unwrap()
    }
    async fn aktion(&self, a: Action) -> Result<(), String> {
        self.state
            .aktion_ausfuehren(a)
            .await
            .map(|_| ())
            .map_err(|e| match e {
                crate::abgleich::AktionFehler::Plan(m) => m,
                crate::abgleich::AktionFehler::Regel(e) => e.message(),
                other => format!("{other:?}"),
            })
    }
    async fn start(&self, plan_first: bool) {
        self.aktion(Action::StartAgent {
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
    async fn warte(&self, was: &str, bedingung: impl Fn(&Workspace) -> bool) {
        let ende = Instant::now() + Duration::from_secs(15);
        loop {
            self.state.aufpassen().await;
            if bedingung(&self.ws()) {
                return;
            }
            assert!(
                Instant::now() < ende,
                "Zeitüberschreitung beim Warten auf: {was}\n{:#?}",
                self.lauf()
            );
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    async fn warte_auf_ende(&self) {
        let ende = Instant::now() + Duration::from_secs(15);
        while !sperre(&self.state.inner.agent.laufend).is_empty() {
            self.state.aufpassen().await;
            assert!(Instant::now() < ende, "der Lauf endet nicht");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
}

/// Eine Welt mit allem eingerichtet. `echt = false` lässt den Container-Teil weg.
fn welt(eingerichtet: bool) -> Welt {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let docker = bin.join("docker");
    std::fs::write(&docker, DOCKER).unwrap();
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Ein Remote mit `main`.
    let quelle = tmp.path().join("quelle");
    std::fs::create_dir_all(&quelle).unwrap();
    sh(&quelle, &["init", "--quiet", "-b", "main"]);
    std::fs::write(quelle.join("README.md"), "hallo\n").unwrap();
    sh(&quelle, &["add", "-A"]);
    sh(&quelle, &["commit", "--quiet", "-m", "Anfang"]);
    std::fs::create_dir_all(tmp.path().join("git/firma")).unwrap();
    sh(
        tmp.path(),
        &["clone", "--quiet", "--bare", "quelle", "git/firma/app.git"],
    );

    let agent_dir = tmp.path().join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(agent_dir.join("Containerfile"), "FROM scratch\n").unwrap();

    let secrets = Arc::new(MemoryStore::new());
    if eingerichtet {
        secrets.set(secrets::LITELLM, &Secret::new(TOKEN)).unwrap();
        secrets.set(secrets::GITHUB, &Secret::new(GITHUB)).unwrap();
    }
    let einst = Einstellungen {
        seed: Seed::Leer,
        container_env: Some("docker".into()),
        suchpfad: Some(bin.clone().into_os_string()),
        agent: AgentWahl {
            anbieter: Provider::ClaudeAgentSdk,
            endpunkt: eingerichtet.then(|| EndpunktKonfig {
                url: Some("https://litellm.example".into()),
                modell: Some("claude-sonnet-5-5".into()),
            }),
        },
        agent_repo: Some("firma/app".into()),
        agent_dir: Some(agent_dir),
        git_basis: Some(format!("file://{}/git", tmp.path().display())),
        daten_dir: Some(tmp.path().join("daten")),
        ..Einstellungen::default()
    };
    let conn = crate::db::open(&tmp.path().join("t.db")).unwrap();
    let state = AppState::neu(
        conn,
        secrets.clone(),
        8787,
        crate::abgleich::Quellen::default(),
        einst,
    );
    state.inner.live.setze_stand_fuer_test(demo::workspace());
    Welt {
        tmp,
        state,
        secrets,
    }
}

const BRANCH: &str = "claude/BAU-142-toggle-fokus-ring-fehlt";

/// Szenario: Der „Container“ ändert eine Datei im Klon, committet und meldet das Ergebnis.
fn szenario_ok() -> String {
    format!(
        r#"echo neu > "$klon/neu.txt"
git -C "$klon" add -A
git -C "$klon" -c user.name=a -c user.email=a@a.invalid commit -qm Agent
printf '%s\n' \
 '{{"t":"schritt","titel":"Datei ändern","text":"neu.txt","token_ein":1000,"token_aus":200,"ms":1500}}' \
 '{{"t":"dateien","dateien":["neu.txt"]}}' \
 '{{"t":"ergebnis","text":"Fokus-Ring ergänzt.","branch":"{BRANCH}","commit":"abc1234"}}'
exit 0
"#
    )
}

/// Ein Zustand wie `welt`, aber mit anderer Wahl von Anbieter und Endpunkt.
fn mit_wahl(w: &Welt, wahl: AgentWahl) -> AppState {
    let e = Einstellungen {
        agent: wahl,
        ..w.state.inner.einstellungen.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("wahl.db")).unwrap();
    let state = AppState::neu(
        conn,
        w.secrets.clone(),
        8787,
        crate::abgleich::Quellen::default(),
        e,
    );
    state.inner.live.setze_stand_fuer_test(demo::workspace());
    state
}

mod container_check;
mod maintenance;
mod pure;
mod run;
mod start;
