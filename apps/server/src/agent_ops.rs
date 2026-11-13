//! Der echte Agent, ausführender Teil: Image bauen, Container starten und überwachen, Ereignisse
//! anwenden, aufräumen. Entscheidungen fallen in `cockpit-core` (Aktionen, Grenzen, Regeln) und in
//! `agent_lauf` (rein); hier laufen nur die Prozesse.
//!
//! Ein Lauf ist ein Container, gestartet mit `docker run` bzw. `podman run` (siehe `agent_lauf`).
//! Das Backend liest dessen stdout zeilenweise, verwirft Unbrauchbares und wendet den Rest als
//! `Action::ReportAgent` (Urheber Agent) an. Stoppen heißt `rm -f` auf den Container.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cockpit_core::api::{AgentStatus, ContainerCheck, ContainerStatus, ImageBuild};
use cockpit_core::{Action, ActionError, Actor, AgentReport, RunKind, RunState, Workspace};
use cockpit_integrations::Secret;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent_git;
use crate::agent_run::{
    self as agent_run, Event, IMAGE, Info, Instruction, LABEL, MAX_LINE, Mode, Prerequisites,
    RunCommand,
};
use crate::app::{AppState, now_ms};
use crate::config::Seed;
use crate::container;
use crate::live::Error;

/// So oft vergleicht der Aufpasser Läufe und Container.
pub const WATCHER_TAKT: Duration = Duration::from_secs(1);
/// So lange gilt „Image vorhanden?“ ohne neue Frage.
const IMAGE_VALID: Duration = Duration::from_secs(5);
/// Nach einem Fehler startet der Aufpasser denselben Lauf so lange nicht neu.
const LOCK_AFTER_ERROR: Duration = Duration::from_secs(30);
/// Zeit über das Limit des Laufs hinaus, bevor das Backend den Container selbst beendet.
const GRACE_S: u64 = 120;
/// Ohne Limit gilt diese Obergrenze für einen Lauf.
const DEFAULT_MAX_S: u64 = 2 * 60 * 60;
/// So lange darf ein arbeitender Container nichts von sich hören lassen.
const QUIET_LIMIT_S: u64 = 15 * 60;
/// Dauer der Erreichbarkeitsprüfung im Container.
const CHECK_TIME: Duration = Duration::from_secs(60);
const MAX_BUILD_LINES: usize = 200;

/// So lange darf der Container höchstens laufen: das Zeitlimit des Laufs (samt Erhöhung) plus
/// etwas Spielraum, damit zuerst die Grenze in `cockpit-core` greift. Ohne Limit gilt eine Obergrenze.
pub fn allowed_seconds(run_minutes: Option<u32>, extra_minutes: u64) -> u64 {
    run_minutes.map_or(DEFAULT_MAX_S, |m| {
        (u64::from(m) + extra_minutes).saturating_mul(60)
    }) + GRACE_S
}

/// Befehle an die Aufgabe eines Laufs.
#[derive(Debug)]
pub enum RunControl {
    /// Container beenden, den Lauf nicht anfassen.
    Stop,
    Pause,
    Resume,
    Answer(String),
    Hint(String),
}

struct Handle {
    info: Arc<Mutex<Info>>,
    tx: mpsc::UnboundedSender<RunControl>,
}

/// Zustand des Agenten im Server.
#[derive(Default)]
pub struct Runs {
    running: Mutex<HashMap<String, Handle>>,
    failed: Mutex<HashMap<String, Instant>>,
    build: Mutex<ImageBuild>,
    image: Mutex<Option<(Instant, bool)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Alles, was ein Lauf an Zugang braucht (aus Einstellungen und Schlüsselbund).
struct Access {
    cli: PathBuf,
    /// `None` = direkt beim Anbieter.
    endpoint_url: Option<String>,
    key: Secret,
    model: String,
    repo: String,
    git_base: String,
    github: Secret,
}

enum End {
    /// Beendet, ohne den Lauf zu ändern (abgebrochen, angehalten).
    Quiet,
    Error(String),
    Done(String),
}

impl AppState {
    /// Läuft der echte Agent (Container) oder die Simulation (Seed `demo`, Tests)?
    pub(crate) fn agent_real(&self) -> bool {
        self.inner.settings.seed != Seed::Demo
    }

    fn search_path(&self) -> OsString {
        self.inner
            .settings
            .search_path
            .clone()
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default()
    }

    /// Die Container-Laufzeit, kurz gemerkt. Blockierend.
    pub(crate) fn container_status(&self) -> ContainerStatus {
        let e = &self.inner.settings;
        let path = self.search_path();
        self.inner
            .container
            .fetch(|| container::detect(e.container.as_deref(), e.container_env.as_deref(), &path))
    }

    fn cli_path(&self, status: &ContainerStatus) -> Option<PathBuf> {
        container::in_path(status.runtime.as_deref()?, &self.search_path())
    }

    /// Gibt es das Agent-Image? (`image inspect`, kurz gemerkt). Blockierend.
    fn image_present(&self, cli: &Path) -> bool {
        let mut c = lock(&self.inner.agent.image);
        if let Some((t, v)) = *c
            && t.elapsed() < IMAGE_VALID
        {
            return v;
        }
        let ok = std::process::Command::new(cli)
            .args(["image", "inspect", IMAGE])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        *c = Some((Instant::now(), ok));
        ok
    }

    fn image_forget(&self) {
        *lock(&self.inner.agent.image) = None;
    }

    /// Der Key des Endpunkts: `agent-key`, ersatzweise das ältere `litellm`.
    fn agent_key(&self) -> Option<Secret> {
        match crate::app::agent_key_read(self.inner.secrets.as_ref()) {
            Ok(k) => k.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Key des Agent-Endpunkts nicht lesbar: {e}");
                None
            }
        }
    }

    fn repo_name(&self) -> Result<String, String> {
        let q = self.sources();
        let github: Vec<String> = q
            .github
            .as_ref()
            .map(|g| g.repos.clone())
            .unwrap_or_default();
        agent_run::repo_choose(self.inner.settings.agent_repo.as_deref(), &github)
            .map(str::to_string)
    }

    /// Der Stand des Agenten für `/api/health`. Blockierend.
    pub(crate) fn agent_status(&self) -> AgentStatus {
        let container = self.container_status();
        let image = self
            .cli_path(&container)
            .filter(|_| container.engine_running)
            .is_some_and(|cli| self.image_present(&cli));
        let choice = self.agent_choice();
        let repo = self.repo_name();
        let v = Prerequisites {
            container: &container,
            image,
            provider: choice.provider,
            model: choice.endpoint.as_ref().is_some_and(|e| e.model.is_some()),
            key: self.agent_key().is_some(),
            repo: repo.as_deref().map_err(Clone::clone),
            github_access: self.github_access().is_some(),
        };
        agent_run::status(self.agent_real(), &v)
    }

    /// Alles für einen Lauf, oder die Liste dessen, was fehlt. Blockierend.
    fn access(&self) -> Result<Access, Vec<String>> {
        let st = self.agent_status();
        if !st.ready {
            return Err(st.missing);
        }
        let container = self.container_status();
        let choice = self.agent_choice();
        if let Some(m) = choice.provider.not_implemented() {
            return Err(vec![m]);
        }
        let endpoint = choice
            .endpoint
            .ok_or_else(|| vec!["Der Modellzugang fehlt.".to_string()])?;
        let q = self.sources();
        let api = q
            .github
            .as_ref()
            .map_or(crate::config::DEFAULT_GITHUB_API, |g| g.api_url.as_str())
            .to_string();
        Ok(Access {
            cli: self.cli_path(&container).ok_or_else(|| {
                vec!["Das Container-Programm ist nicht mehr zu finden.".to_string()]
            })?,
            endpoint_url: endpoint.url,
            key: self
                .agent_key()
                .ok_or_else(|| vec!["Der Key des Modellzugangs fehlt.".to_string()])?,
            model: endpoint
                .model
                .ok_or_else(|| vec!["Der Modellname fehlt.".to_string()])?,
            repo: st.repo.ok_or_else(|| vec!["Das Repo fehlt.".to_string()])?,
            git_base: self
                .inner
                .settings
                .git_base
                .clone()
                .unwrap_or_else(|| agent_git::base_from_api(&api)),
            github: self
                .github_access()
                .ok_or_else(|| vec!["GitHub ist nicht verbunden.".to_string()])?,
        })
    }

    fn agent_dir(&self) -> PathBuf {
        self.inner
            .settings
            .data_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("cockpit"))
            .join("agent")
    }

    /// Der Ordner mit `Containerfile` und Läufer.
    fn agent_source(&self) -> PathBuf {
        self.inner
            .settings
            .agent_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("apps/agent"))
    }

    /// Ablehnungen vor dem Anwenden einer Aktion im echten Modus: Läufe starten nur, wenn alles
    /// eingerichtet ist, und Folgeaufträge gibt es noch nicht. `None`, wenn nichts dagegen spricht.
    /// Blockierend.
    pub(crate) fn agent_lock(&self, action: &Action) -> Option<String> {
        if !self.agent_real() {
            return None;
        }
        match action {
            Action::StartAgent { .. } | Action::ResumeStopped { .. } => {
                let st = self.agent_status();
                (!st.ready).then(|| {
                    format!(
                        "Der Agent kann noch nicht starten. {}",
                        st.missing.join(" ")
                    )
                })
            }
            Action::ApprovePlan { ticket } => {
                let ws = self.inner.live.copy();
                let waits = ws.runs.iter().any(|r| {
                    r.key() == *ticket && r.is_awaiting() && r.agent_plan.is_none()
                });
                waits.then(|| {
                    "Der Agent erarbeitet den Plan noch. Warte, bis er vorliegt – dann kannst du ihn freigeben."
                        .to_string()
                })
            }
            Action::AddressReview { .. } | Action::InvestigateCheck { .. } | Action::ApplyFix { .. } => {
                Some(
                    "Folgeaufträge (Kommentare umsetzen, Checks untersuchen) laufen noch nicht mit dem echten Agenten. Setze die Änderungen vorerst selbst um."
                        .to_string(),
                )
            }
            _ => None,
        }
    }

    // ---------- Meldungen anwenden ----------

    /// Wendet eine Meldung des Agenten an. Blockierend.
    fn report_blocking(&self, key: &str, report: AgentReport) -> Result<(), Error> {
        self.inner
            .live
            .apply(
                &self.inner.db,
                Action::ReportAgent {
                    run: key.to_string(),
                    report,
                },
                Actor::Agent,
                now_ms(),
            )
            .map(|_| ())
    }

    async fn report(&self, key: &str, report: AgentReport) -> Result<(), Error> {
        let s = self.clone();
        let k = key.to_string();
        tokio::task::spawn_blocking(move || s.report_blocking(&k, report))
            .await
            .unwrap_or_else(|_| Err(Error::Save("Interner Fehler".into())))
    }
}

mod container_check;
mod image_build;
mod lines;
mod run;
mod watcher;

#[cfg(test)]
pub use container_check::check_hint;
use container_check::{cli_still, container_labels_remove, container_remove, kind_env, last_line};
#[cfg(test)]
pub use image_build::build_hint;
pub use lines::{Read, read_line};
use run::run_task;
pub use watcher::watcher_start;

#[cfg(test)]
mod tests;
