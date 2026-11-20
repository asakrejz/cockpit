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

use cockpit_core::api::{
    AgentStatus, ContainerCheck, ContainerStatus, ImageBuild, ImageState, Provider,
};
use cockpit_core::{
    Action, ActionError, Actor, AgentReport, AgentRepos, FollowupTask, RepoChoice, RunKind,
    RunState, Workspace,
};
use cockpit_i18n::{ArgValue, Msg, msg};
use cockpit_integrations::Secret;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent_git;
use crate::agent_image;
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

/// Result of comparing the image with the sources, remembered briefly.
#[derive(Clone, Debug)]
struct ImageProbe {
    state: ImageState,
    /// Plain-text reason when `state` is `Unknown`.
    hint: Option<Msg>,
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
    image: Mutex<Option<(Instant, ImageProbe)>>,
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
    git_base: String,
    github: Secret,
}

enum End {
    /// Ended without changing the run (cancelled, halted).
    Quiet,
    Error(Msg),
    Done(String),
}

impl AppState {
    /// The state of the agent for `/api/health`. Blocking.
    pub(crate) fn agent_status(&self) -> AgentStatus {
        let container = self.container_status();
        let probe = self
            .cli_path(&container)
            .filter(|_| container.engine_running)
            .map(|cli| self.image_probe(&cli))
            .unwrap_or(ImageProbe {
                state: ImageState::Missing,
                hint: None,
            });
        let choice = self.agent_choice();
        let repos = self.agent_repos();
        let v = Prerequisites {
            container: &container,
            image: probe.state,
            image_hint: probe.hint,
            model: choice.endpoint.as_ref().is_some_and(|e| e.model.is_some()),
            key: self.agent_key().is_some(),
            github_access: self.github_access().is_some(),
        };
        let mut status = agent_run::status(self.agent_real(), &v);
        // The interface preselects the repo with the same rules the backend applies. Without a
        // configured repo the user chooses one when starting; that is no missing prerequisite.
        status.repo = repos.default.clone();
        status.repos = repos;
        status
    }

    /// Everything for a run, or the list of what is missing. Blocking.
    fn access(&self) -> Result<Access, Vec<Msg>> {
        let st = self.agent_status();
        if !st.ready {
            return Err(st.missing_msgs);
        }
        let container = self.container_status();
        let choice = self.agent_choice();
        let endpoint = choice
            .endpoint
            .ok_or_else(|| vec![msg!(ServerRunAccessNoEndpoint)])?;
        let q = self.sources();
        let api = q
            .github
            .as_ref()
            .map_or(crate::config::DEFAULT_GITHUB_API, |g| g.api_url.as_str())
            .to_string();
        Ok(Access {
            provider: choice.provider,
            cli: self
                .cli_path(&container)
                .ok_or_else(|| vec![msg!(ServerRunAccessNoCli)])?,
            endpoint_url: endpoint.url,
            key: self
                .agent_key()
                .ok_or_else(|| vec![msg!(ServerRunAccessNoKey)])?,
            model: endpoint
                .model
                .ok_or_else(|| vec![msg!(ServerRunAccessNoModel)])?,
            git_base: self
                .inner
                .settings
                .git_base
                .clone()
                .unwrap_or_else(|| agent_git::base_from_api(&api)),
            github: self
                .github_access()
                .ok_or_else(|| vec![msg!(ServerRunAccessNoGithub)])?,
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
    pub(crate) fn agent_lock(&self, action: &Action) -> Option<Msg> {
        if !self.agent_real() {
            // The follow-up jobs and the pull request belong to the real agent.
            return matches!(
                action,
                Action::AddressReviewThreads { .. }
                    | Action::FixFailedChecks { .. }
                    | Action::StartPrReview { .. }
                    | Action::CreatePullRequest { .. }
            )
            .then(|| msg!(ServerAgentOnlyRealAgent));
        }
        match action {
            Action::StartAgent { .. }
            | Action::ResumeStopped { .. }
            | Action::AddressReviewThreads { .. }
            | Action::FixFailedChecks { .. } => {
                let st = self.agent_status();
                (!st.ready).then(|| cannot_start(&st))
            }
            Action::StartPrReview { review, .. } => {
                let st = self.agent_status();
                if !st.ready {
                    return Some(cannot_start(&st));
                }
                // The pull request must be synced and name a repo the agent can work in.
                let ws = self.inner.live.copy();
                let Some(link) = ws.sync.pr(review) else {
                    return Some(msg!(ServerAgentPrNotSynced, review = review.to_string()));
                };
                (!self.repo_usable(&link.repo)).then(|| {
                    msg!(
                        ServerAgentPrOtherRepo,
                        review = review.to_string(),
                        repo = &link.repo
                    )
                })
            }
            Action::ApprovePlan { ticket } => {
                let ws = self.inner.live.copy();
                let waits = ws
                    .runs
                    .iter()
                    .any(|r| r.key() == *ticket && r.is_awaiting() && r.agent_plan.is_none());
                waits.then(|| msg!(ServerAgentPlanPending))
            }
            Action::AddressReview { .. }
            | Action::InvestigateCheck { .. }
            | Action::ApplyFix { .. } => Some(msg!(ServerAgentFollowupUnknown)),
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
            .unwrap_or_else(|_| Err(Error::Save("Internal error".into())))
    }
}

/// "The agent cannot start yet" with the list of what is missing.
fn cannot_start(st: &AgentStatus) -> Msg {
    msg!(
        ServerAgentCannotStart,
        missing = ArgValue::joined(st.missing_msgs.clone(), " ")
    )
}

mod branch_delete;
mod cleanup;
mod container_check;
mod image_build;
mod lines;
mod probe;
mod repos;
mod run;
pub mod suggest;
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
