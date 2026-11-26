use cockpit_i18n::{ArgValue, Lang, Msg, msg};

use super::*;

mod prepare;
mod publish;
mod stream;

use prepare::{Prepared, Target, prepare};
use publish::{publish_repos, publish_single};
use stream::{container_start, exit_code, exit_reason, halt, report_started, watch};

// ---------- A run ----------

pub(super) async fn run_task(
    state: AppState,
    key: String,
    mode: Mode,
    mut rx: mpsc::UnboundedReceiver<RunControl>,
    info: Arc<Mutex<Info>>,
) {
    // Set once a container exists that the backend has to remove itself (strategy "copy": no `--rm`).
    let mut created: Option<(PathBuf, String)> = None;
    let end = run_run(&state, &key, mode, &mut rx, &info, &mut created).await;
    if let Some((cli, name)) = created {
        container_remove(&cli, &name).await;
    }
    match end {
        End::Quiet => {}
        End::Error(reason) => {
            tracing::warn!("Run {key} ends with error: {}", reason.render(Lang::En));
            lock(&state.inner.agent.failed).insert(key.clone(), Instant::now());
            if let Err(e) = state.report(&key, AgentReport::Failed { reason }).await {
                tracing::debug!("Error of run {key} can no longer be reported: {e:?}");
            }
        }
        End::Done(summary) => {
            if let Err(e) = state.report(&key, AgentReport::Done { summary }).await {
                tracing::debug!("Result of run {key} can no longer be reported: {e:?}");
            }
        }
    }
    // Clean up: the clone only stays while the run waits halted by a limit.
    let halted = state
        .inner
        .live
        .copy()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_stopped());
    let dir = state
        .agent_dir()
        .join("runs")
        .join(agent_run::run_dir(&key));
    if mode == Mode::Plan {
        let _ = tokio::fs::remove_dir_all(dir.join("plan")).await;
    } else if mode == Mode::Review || !halted {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    lock(&state.inner.agent.running).remove(&key);
}

pub(super) enum Application {
    Ok,
    /// The run is no longer active or the message does not (or no longer) fit.
    Rejected,
    Error,
}

pub(super) async fn apply(state: &AppState, key: &str, r: AgentReport) -> Application {
    match state.report(key, r).await {
        Ok(()) => Application::Ok,
        Err(Error::Action(ActionError::NothingToDo)) => Application::Rejected,
        Err(Error::Action(_)) => Application::Rejected,
        Err(Error::Save(m)) => {
            tracing::warn!("Message not saved: {m}");
            Application::Error
        }
    }
}

pub(super) fn command_line(t: &str, text: &str) -> String {
    format!("{}\n", serde_json::json!({ "t": t, "text": text }))
}

pub(super) async fn run_run(
    state: &AppState,
    key: &str,
    mode: Mode,
    rx: &mut mpsc::UnboundedReceiver<RunControl>,
    info: &Arc<Mutex<Info>>,
    created: &mut Option<(PathBuf, String)>,
) -> End {
    let ws: Workspace = state.inner.live.copy();
    let Some(run) = ws.run(key) else {
        return End::Quiet;
    };
    state.oauth_refresh().await;
    let s = state.clone();
    let access = match tokio::task::spawn_blocking(move || s.access()).await {
        Ok(Ok(z)) => z,
        Ok(Err(missing)) => {
            return End::Error(msg!(
                ServerRunCannotStart,
                reasons = ArgValue::joined(missing, " ")
            ));
        }
        Err(_) => return End::Error(msg!(ServerRunInternalPrepare)),
    };
    // The repo this run works in (always one of the configured ones).
    let repo = match state.run_repo(&run) {
        Ok(r) => r,
        Err(m) => return End::Error(m),
    };
    let mut run = run;
    run.repo.clone_from(&repo);
    let secret = [
        access.key.expose().to_string(),
        access.github.expose().to_string(),
    ];
    let secret_refs: Vec<&str> = secret.iter().map(String::as_str).collect();

    // Clone the repos and write the job file.
    let prepared = match prepare(state, key, mode, &ws, &run, &access, &secret_refs).await {
        Ok(p) => p,
        Err(m) => return End::Error(m),
    };
    let (branch, multi) = (prepared.branch.clone(), prepared.multi);
    let clone = prepared.clone.clone();

    // Start the container.
    let mut container =
        match container_start(&access, key, mode, &clone, &prepared.job_dir, created).await {
            Ok(c) => c,
            Err(m) => return End::Error(m),
        };
    report_started(state, key, mode, &run, &access).await;

    // Time limit of the run (run time of the agent, without pauses and waiting time for you).
    let allowed_s = state
        .inner
        .settings
        .run_seconds
        .unwrap_or_else(|| allowed_seconds(ws.limits.run_minutes, run.extra_minutes));
    let watched = watch(state, key, mode, rx, info, &mut container, allowed_s).await;
    if watched.quiet || watched.too_much.is_some() {
        halt(&mut container, mode, &clone).await;
        return match watched.too_much {
            Some(m) => End::Error(m),
            None => End::Quiet,
        };
    }

    // The stream has ended: how did the container end?
    let code = exit_code(&mut container).await;
    if mode == Mode::Plan && watched.plan_reported {
        return End::Quiet;
    }
    let Some((text, result_branch)) = watched.result else {
        return End::Error(exit_reason(
            watched.error_text,
            code,
            &container.stderr_end,
            &secret_refs,
        ));
    };
    if mode == Mode::Plan {
        return End::Error(msg!(ServerRunPlanGotResult));
    }
    if mode == Mode::Review {
        // Nothing to push: the drafts are already booked on the review.
        return End::Done(Msg::raw(text));
    }
    if !multi && result_branch != branch {
        return End::Error(msg!(
            ServerRunBranchMismatch,
            got = result_branch,
            expected = &branch
        ));
    }

    // Strategy "copy": the result is in the container, not in the clone yet. The backend's own
    // checks and the push below work on the copied-out clone, exactly as with a bind mount.
    if container.copy
        && let Err(m) = copy_io::fetch(&container.cli, &container.name, &clone).await
    {
        return End::Error(m);
    }

    // Check and push – the backend, not the container.
    if !multi {
        return publish_single(state, key, &run, prepared, text, &access).await;
    }
    publish_repos(
        state,
        key,
        &run,
        prepared.targets,
        &watched.result_repos,
        &text,
        &access,
    )
    .await
}

pub(super) fn run_active(state: &AppState, key: &str) -> bool {
    state
        .inner
        .live
        .copy()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_active())
}

/// The run is no longer active or was halted by a limit: the container should end.
pub(super) fn run_halted_or_gone(state: &AppState, key: &str) -> bool {
    !state
        .inner
        .live
        .copy()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_active() && !r.is_stopped())
}
