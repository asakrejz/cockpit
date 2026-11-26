use tokio::process::{Child, ChildStdin, ChildStdout};

use super::*;

/// The container of a run with the ends of its pipes.
pub(super) struct Container {
    pub cli: PathBuf,
    pub name: String,
    /// Strategy "copy": clone and job go in and out by copying, not by bind mounts.
    pub copy: bool,
    pub kind: Child,
    pub stdin: Option<ChildStdin>,
    pub stdout: BufReader<ChildStdout>,
    /// The last lines of stderr, for the message when the container ends without a result.
    pub stderr_end: Arc<Mutex<VecDeque<String>>>,
}

/// What the stream of the container left once it ended.
pub(super) struct Watched {
    /// The result of the runner: text and branch.
    pub result: Option<(String, String)>,
    pub error_text: Option<String>,
    pub result_repos: Vec<agent_run::ResultRepo>,
    pub plan_reported: bool,
    /// Ended without changing the run (cancelled, halted, no longer active).
    pub quiet: bool,
    /// Ended by a limit of the run (run time, silence).
    pub too_much: Option<Msg>,
}

/// Creates and starts the container (strategy "copy": create, copy clone and job in, then start it
/// attached). `created` names the container the backend has to remove itself (no `--rm`).
pub(super) async fn container_start(
    access: &Access,
    key: &str,
    mode: Mode,
    clone: &Path,
    job_dir: &Path,
    created: &mut Option<(PathBuf, String)>,
) -> Result<Container, Msg> {
    let name = agent_run::container_name(key, now_ms());
    let args = agent_run::run_args(&RunCommand {
        name: &name,
        run: key,
        clone,
        // Plan and review runs only read: the clone is mounted `:ro`.
        clone_only_read: mode != Mode::Implement,
        job: job_dir,
        stdin_open: true,
        provider: access.provider,
        io: access.io,
    });
    let mut cmd = Command::new(&access.cli);
    cmd.args(&args);
    kind_env(
        &mut cmd,
        access.provider,
        access.endpoint_url.as_deref(),
        access.key.expose(),
        &access.model,
    );
    let copy = access.io == agent_run::AgentIo::Copy;
    if copy {
        // No mounts: create the container, copy clone and job in, then start it attached.
        copy_io::prepare(&access.cli, cmd, &name, clone, job_dir).await?;
        *created = Some((access.cli.clone(), name.clone()));
        cmd = Command::new(&access.cli);
        cmd.args(agent_run::start_args(&name, true));
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut kind = match cmd.spawn() {
        Ok(k) => k,
        Err(e) => {
            return Err(msg!(
                ServerRunContainerNotStartedHint,
                error = e.to_string()
            ));
        }
    };
    let cli = access.cli.clone();
    let stdin = kind.stdin.take();
    let stdout = BufReader::new(kind.stdout.take().expect("stdout is piped"));
    let stderr_end: Arc<Mutex<VecDeque<String>>> = Arc::default();
    if let Some(se) = kind.stderr.take() {
        let end = stderr_end.clone();
        tokio::spawn(async move {
            let mut r = BufReader::new(se);
            let mut buffer = Vec::new();
            while let Ok(Some(Read::Line(z))) = read_line(&mut r, &mut buffer).await {
                let mut e = lock(&end);
                e.push_back(z);
                if e.len() > 20 {
                    e.pop_front();
                }
            }
        });
    }
    Ok(Container {
        cli,
        name,
        copy,
        kind,
        stdin,
        stdout,
        stderr_end,
    })
}

/// Books "started" on the run, with the runtime, what kind of run it is and the model.
pub(super) async fn report_started(
    state: &AppState,
    key: &str,
    mode: Mode,
    run: &cockpit_core::AgentRun,
    access: &Access,
) {
    let runtime = access
        .cli
        .file_name()
        .map_or_else(|| "container".into(), |n| n.to_string_lossy().into_owned());
    let _ = apply(
        state,
        key,
        AgentReport::Started {
            detail: msg!(
                ServerRunStartedDetail,
                runtime = runtime,
                what = match (mode, &run.kind) {
                    (Mode::Plan, _) => msg!(ServerRunModePlan),
                    (Mode::Review, _) => msg!(ServerRunModeReview),
                    (Mode::Implement, RunKind::Followup { task, .. }) => match task {
                        FollowupTask::ReviewThreads => msg!(ServerRunModeReviewComments),
                        FollowupTask::FailedChecks { .. } => msg!(ServerRunModeFailedChecks),
                    },
                    (Mode::Implement, _) => msg!(ServerRunModeTicket),
                },
                model = &access.model
            ),
        },
    )
    .await;
}

/// Reads the container's lines, applies them as messages of the agent, passes on the commands of
/// the user and watches the limits, until the stream ends or the run is stopped.
pub(super) async fn watch(
    state: &AppState,
    key: &str,
    mode: Mode,
    rx: &mut mpsc::UnboundedReceiver<RunControl>,
    info: &Arc<Mutex<Info>>,
    container: &mut Container,
    allowed_s: u64,
) -> Watched {
    let (mut active_s, mut still_s) = (0u64, 0u64);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut buffer = Vec::new();
    let mut discarded = 0usize;
    let mut result: Option<(String, String)> = None;
    let mut error_text: Option<String> = None;
    let mut result_repos: Vec<agent_run::ResultRepo> = Vec::new();
    let mut plan_reported = false;
    let mut quiet = false;
    let mut too_much: Option<Msg> = None;

    loop {
        tokio::select! {
            read = read_line(&mut container.stdout, &mut buffer) => {
                let line = match read {
                    Ok(Some(Read::Line(z))) => z,
                    Ok(Some(Read::TooLarge)) => { discarded += 1; continue; }
                    Ok(None) | Err(_) => break,
                };
                still_s = 0;
                let e = match agent_run::line_read(&line) {
                    Ok(e) => e,
                    Err(_) => { discarded += 1; continue; }
                };
                match &e {
                    Event::Result { text, branch: b, repos, .. } => {
                        result = Some((text.clone(), b.clone()));
                        result_repos = repos.clone();
                    }
                    Event::Error { text } => { error_text = Some(text.clone()); }
                    Event::Question { .. } if mode == Mode::Implement => {
                        lock(info).question_open = true;
                    }
                    Event::HintsArrived => {
                        let mut i = lock(info);
                        i.hints_sent = i.hints_sent.saturating_sub(1);
                    }
                    _ => {}
                }
                if let Some(message) = agent_run::as_message(&e, mode) {
                    let is_plan = matches!(message, AgentReport::Plan { .. });
                    let is_question = matches!(message, AgentReport::Question { .. });
                    match apply(state, key, message).await {
                        Application::Ok => { plan_reported |= is_plan; }
                        Application::Rejected => {
                            if is_question { lock(info).question_open = false; }
                            // The run is no longer active or no longer waits for it.
                            if !run_active(state, key) { quiet = true; break; }
                        }
                        Application::Error => {}
                    }
                    // A limit (budget, time) may have triggered this message.
                    if !quiet && run_halted_or_gone(state, key) { quiet = true; break; }
                }
            }
            Some(b) = rx.recv() => {
                match b {
                    RunControl::Stop => { quiet = true; break; }
                    RunControl::Pause => { cli_still(&container.cli, &["pause", &container.name]).await; lock(info).paused = true; }
                    RunControl::Resume => { cli_still(&container.cli, &["unpause", &container.name]).await; lock(info).paused = false; }
                    RunControl::Answer(t) => {
                        if let Some(s) = container.stdin.as_mut() { let _ = s.write_all(command_line("answer", &t).as_bytes()).await; let _ = s.flush().await; }
                        lock(info).question_open = false;
                    }
                    RunControl::Hint(t) => {
                        if let Some(s) = container.stdin.as_mut() { let _ = s.write_all(command_line("hint", &t).as_bytes()).await; let _ = s.flush().await; }
                        lock(info).hints_sent += 1;
                    }
                }
            }
            _ = ticker.tick() => {
                let (paused, asks) = { let i = lock(info); (i.paused, i.question_open) };
                if !paused && !asks {
                    active_s += 1;
                    still_s += 1;
                }
                if active_s > allowed_s {
                    too_much = Some(msg!(ServerRunTimeLimit, minutes = allowed_s / 60));
                    break;
                }
                if still_s > QUIET_LIMIT_S {
                    too_much = Some(msg!(ServerRunQuietLimit, minutes = QUIET_LIMIT_S / 60));
                    break;
                }
            }
        }
    }
    if discarded > 0 {
        tracing::warn!("Run {key}: {discarded} unusable container lines discarded");
    }
    drop(container.stdin.take());
    Watched {
        result,
        error_text,
        result_repos,
        plan_reported,
        quiet,
        too_much,
    }
}

/// Ends the container after a stop or a limit.
pub(super) async fn halt(container: &mut Container, mode: Mode, clone: &Path) {
    // A halted run resumes from the clone: take over what the agent did so far.
    if container.copy && mode == Mode::Implement {
        let _ = copy_io::fetch(&container.cli, &container.name, clone).await;
    }
    container_remove(&container.cli, &container.name).await;
    let _ = tokio::time::timeout(Duration::from_secs(10), container.kind.wait()).await;
}

/// The stream has ended: how did the container end? The exit code, `None` if it was killed.
pub(super) async fn exit_code(container: &mut Container) -> Option<i32> {
    let status = match tokio::time::timeout(Duration::from_secs(30), container.kind.wait()).await {
        Ok(s) => s.ok(),
        Err(_) => {
            container_remove(&container.cli, &container.name).await;
            let _ = container.kind.wait().await;
            None
        }
    };
    status.and_then(|s| s.code())
}

/// Why a container that ended without a result failed, from its own error text or its exit code.
pub(super) fn exit_reason(
    error_text: Option<String>,
    code: Option<i32>,
    stderr_end: &Mutex<VecDeque<String>>,
    secret_refs: &[&str],
) -> Msg {
    let still_end = || {
        lock(stderr_end)
            .iter()
            .rev()
            .map(|z| z.trim())
            .find(|z| !z.is_empty())
            .and_then(|z| last_line(z, secret_refs))
    };
    // The text of the container comes from outside: it has no code.
    error_text
        .map(|t| Msg::raw(agent_git::redact(&t, secret_refs)))
        .unwrap_or_else(|| {
            let with = |head: Msg, tail: Option<Msg>| match tail {
                Some(t) => msg!(ServerAgentJoined, parts = ArgValue::joined([head, t], " ")),
                None => head,
            };
            match code {
                Some(137) => msg!(ServerRunExit137),
                Some(c @ 125..=127) => with(
                    msg!(ServerRunExitNotStarted, code = c),
                    still_end().map(|z| msg!(ServerRunExitMessage, line = z)),
                ),
                Some(0) => msg!(ServerRunExitNoResult),
                Some(c) => with(
                    msg!(ServerRunExitCode, code = c),
                    still_end().map(|z| msg!(ServerRunLastMessage, line = z)),
                ),
                None => msg!(ServerRunExitKilled),
            }
        })
}
