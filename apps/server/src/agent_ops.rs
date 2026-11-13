//! The real agent, executing part: build the image, start and watch containers, apply
//! events, clean up. Decisions are made in `cockpit-core` (actions, limits, rules) and in
//! `agent_run` (pure); here only the processes run.
//!
//! A run is a container started with `docker run` or `podman run` (see `agent_run`).
//! The backend reads its stdout line by line, discards what is unusable and applies the rest as
//! `Action::ReportAgent` (author agent). Stopping means `rm -f` on the container.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cockpit_core::api::{AgentStatus, ContainerCheck, ContainerStatus, ImageBuild, Provider};
use cockpit_core::{
    Action, ActionError, Actor, AgentReport, FollowupTask, RunKind, RunState, Workspace,
};
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

/// This often the watcher compares runs and containers.
pub const WATCHER_TICK: Duration = Duration::from_secs(1);
/// This long "image present?" applies without a new question.
const IMAGE_VALID: Duration = Duration::from_secs(5);
/// After an error the watcher does not restart the same run for this long.
const LOCK_AFTER_ERROR: Duration = Duration::from_secs(30);
/// Time beyond the limit of the run before the backend ends the container itself.
const GRACE_S: u64 = 120;
/// Without a limit this upper bound applies for a run.
const DEFAULT_MAX_S: u64 = 2 * 60 * 60;
/// This long a working container may not let hear anything from itself.
const QUIET_LIMIT_S: u64 = 15 * 60;
/// Duration of the reachability check in the container.
const CHECK_TIME: Duration = Duration::from_secs(60);
const MAX_BUILD_LINES: usize = 200;

/// This long the container may run at most: the time limit of the run (including the raise) plus
/// some leeway so that the limit in `cockpit-core` takes effect first. Without a limit an upper bound applies.
pub fn allowed_seconds(run_minutes: Option<u32>, extra_minutes: u64) -> u64 {
    run_minutes.map_or(DEFAULT_MAX_S, |m| {
        (u64::from(m) + extra_minutes).saturating_mul(60)
    }) + GRACE_S
}

/// Commands to the task of a run.
#[derive(Debug)]
pub enum RunControl {
    /// End the container, do not touch the run.
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

/// State of the agent in the server.
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

/// Everything a run needs in terms of access (from settings and .env file).
struct Access {
    cli: PathBuf,
    provider: Provider,
    /// `None` = directly at the provider.
    endpoint_url: Option<String>,
    key: Secret,
    model: String,
    repo: String,
    git_base: String,
    github: Secret,
}

enum End {
    /// Ended without changing the run (cancelled, halted).
    Quiet,
    Error(String),
    Done(String),
}

impl AppState {
    /// Does the real agent (container) run or the simulation (seed `demo`, tests)?
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

    /// The container runtime, remembered briefly. Blocking.
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

    /// Is there the agent image? (`image inspect`, remembered briefly). Blocking.
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

    /// The key of the endpoint: `agent-key`, alternatively the older `litellm`.
    fn agent_key(&self) -> Option<Secret> {
        match crate::app::agent_key_read(self.inner.secrets.as_ref()) {
            Ok(k) => k.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Agent endpoint key not readable: {e}");
                None
            }
        }
    }

    pub(crate) fn repo_name(&self) -> Result<String, String> {
        let q = self.sources();
        let github: Vec<String> = q
            .github
            .as_ref()
            .map(|g| g.repos.clone())
            .unwrap_or_default();
        agent_run::repo_choose(self.inner.settings.agent_repo.as_deref(), &github)
            .map(str::to_string)
    }

    /// The state of the agent for `/api/health`. Blocking.
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
            model: choice.endpoint.as_ref().is_some_and(|e| e.model.is_some()),
            key: self.agent_key().is_some(),
            repo: repo.as_deref().map_err(Clone::clone),
            github_access: self.github_access().is_some(),
        };
        agent_run::status(self.agent_real(), &v)
    }

    /// Everything for a run, or the list of what is missing. Blocking.
    fn access(&self) -> Result<Access, Vec<String>> {
        let st = self.agent_status();
        if !st.ready {
            return Err(st.missing);
        }
        let container = self.container_status();
        let choice = self.agent_choice();
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
            provider: choice.provider,
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

    /// The folder with `Containerfile` and runner.
    fn agent_source(&self) -> PathBuf {
        self.inner
            .settings
            .agent_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("apps/agent"))
    }

    /// Rejections before applying an action in real mode: runs only start when everything
    /// is set up, and the simulated follow-up jobs do not exist with the real agent. `None` if nothing
    /// speaks against it. Blocking.
    pub(crate) fn agent_lock(&self, action: &Action) -> Option<String> {
        if !self.agent_real() {
            // The follow-up jobs and the pull request belong to the real agent.
            return matches!(
                action,
                Action::AddressReviewThreads { .. }
                    | Action::FixFailedChecks { .. }
                    | Action::CreatePullRequest { .. }
            )
            .then(|| {
                "Das gibt es nur mit dem echten Agenten. Im Demo-Modus laufen die Folgeaufträge als Simulation direkt am Review."
                    .to_string()
            });
        }
        match action {
            Action::StartAgent { .. }
            | Action::ResumeStopped { .. }
            | Action::AddressReviewThreads { .. }
            | Action::FixFailedChecks { .. } => {
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
                    "Diesen Folgeauftrag gibt es mit dem echten Agenten nicht. Nutze am Review „Review-Kommentare umsetzen“ oder „Rote Checks beheben“."
                        .to_string(),
                )
            }
            _ => None,
        }
    }

    // ---------- Applying messages ----------

    /// Applies a message of the agent. Blocking.
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
