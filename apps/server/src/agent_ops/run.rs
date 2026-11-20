use cockpit_i18n::{ArgValue, Lang, Msg, msg};

use super::*;

// ---------- A run ----------

pub(super) fn job_write(dir: &Path, content: &serde_json::Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join("job.json");
    std::fs::write(&file, content.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

pub(super) async fn run_task(
    state: AppState,
    key: String,
    mode: Mode,
    mut rx: mpsc::UnboundedReceiver<RunControl>,
    info: Arc<Mutex<Info>>,
) {
    let end = run_run(&state, &key, mode, &mut rx, &info).await;
    match end {
        End::Quiet => {}
        End::Error(reason) => {
            tracing::warn!("Run {key} ends with error: {}", reason.render(Lang::En));
            lock(&state.inner.agent.failed).insert(key.clone(), Instant::now());
            // The report of the agent carries plain text (the workspace log has no codes there yet).
            let reason = reason.de();
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

#[allow(clippy::too_many_lines)]
pub(super) async fn run_run(
    state: &AppState,
    key: &str,
    mode: Mode,
    rx: &mut mpsc::UnboundedReceiver<RunControl>,
    info: &Arc<Mutex<Info>>,
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

    // Follow-up job: the inputs come from GitHub (read only, with the token of the person) and go
    // into the job file as text. Whatever looks like one of our keys is removed first.
    let followup = match state.followup_job(&ws, &run.kind).await {
        Ok(Some(job)) => {
            let text = agent_git::redact(&job.to_json().to_string(), &secret_refs);
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .map(|v| (job, v))
        }
        Ok(None) => None,
        Err(m) => return End::Error(m),
    };

    let branch = run.branch();
    let base_dir = state.agent_dir().join("runs").join(agent_run::run_dir(key));
    let mode_dir = base_dir.join(match mode {
        Mode::Plan => "plan",
        Mode::Review => "review",
        Mode::Implement => "run",
    });
    // A run in several repos (only "implement ticket" and its plan): every repo gets its own
    // clone in `<mode>/<owner>__<name>`, and the container sees `/work/<owner>__<name>`. A run in
    // one repo keeps the clone directly in `/work`.
    let multi = !run.is_followup() && mode != Mode::Review && run.repos.len() > 1;
    let mut targets: Vec<Target> = Vec::new();
    if multi {
        for entry in &run.repos {
            let name = match state.run_repo_of(&run, &entry.repo) {
                Ok(r) => r,
                Err(m) => return End::Error(m),
            };
            let dir = cockpit_core::repo_dir(&name);
            targets.push(Target {
                branch: run.branch_in(&name),
                // The pattern comes from the configuration of the repo, never from the run's own options.
                pattern: state.agent_repos().branch_pattern(&name),
                base: if entry.base_branch.is_empty() {
                    run.options.base_branch.clone()
                } else {
                    entry.base_branch.clone()
                },
                url: agent_git::clone_url(&access.git_base, &name),
                clone: mode_dir.join(&dir),
                base_file: base_dir.join(format!("base-{dir}")),
                export: base_dir.join(format!("export-{dir}.git")),
                rules: entry.rules.clone(),
                base_sha: String::new(),
                repo: name,
            });
        }
    } else {
        targets.push(Target {
            branch: branch.clone(),
            // The pattern comes from the configuration of the repo, never from the run's own options.
            pattern: state.agent_repos().branch_pattern(&repo),
            base: run.options.base_branch.clone(),
            url: agent_git::clone_url(&access.git_base, &repo),
            clone: mode_dir.clone(),
            base_file: base_dir.join("base"),
            export: base_dir.join("export.git"),
            rules: run.rules.clone(),
            base_sha: String::new(),
            repo: repo.clone(),
        });
    }
    let clone = mode_dir.clone();

    // Clone: fresh, unless a halted run is resumed.
    let present = mode == Mode::Implement
        && targets
            .iter()
            .all(|t| t.clone.join(".git").is_dir() && t.base_file.is_file());
    for t in &mut targets {
        if present {
            t.base_sha = std::fs::read_to_string(&t.base_file)
                .unwrap_or_default()
                .trim()
                .to_string();
            continue;
        }
        let (k, u, tk, b, br, pt) = (
            t.clone.clone(),
            t.url.clone(),
            access.github.clone(),
            t.base.clone(),
            t.branch.clone(),
            t.pattern.clone(),
        );
        let on_existing_branch = run.kind.review().is_some();
        let read_only_review = mode == Mode::Review;
        let bd = t.base_file.clone();
        let cloned = tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&k);
            if let Some(p) = k.parent() {
                std::fs::create_dir_all(p)
                    .map_err(|e| msg!(ServerRunDirNotCreatable, error = e.to_string()))?;
            }
            // A follow-up job continues on the branch of the pull request; a new run branches off.
            let rule = agent_git::BranchRule {
                branch: &br,
                pattern: &pt,
                base: &b,
                existing: on_existing_branch,
            };
            let sha = if read_only_review {
                // Any branch of the pull request: the agent only reads this clone.
                agent_git::clone_for_review(&u, &k, Some(&tk), &br)?
            } else if on_existing_branch {
                agent_git::clone_branch(&u, &k, Some(&tk), &rule)?
            } else {
                agent_git::clone(&u, &k, Some(&tk), &rule)?
            };
            std::fs::write(&bd, &sha)
                .map_err(|e| msg!(ServerRunStateNotSaved, error = e.to_string()))?;
            Ok::<_, Msg>(sha)
        })
        .await;
        match cloned {
            Ok(Ok(sha)) => t.base_sha = sha,
            Ok(Err(m)) => {
                // The git errors are redacted where they are made.
                return End::Error(msg!(ServerRunCloneFailed, repo = &t.repo, reason = m));
            }
            Err(_) => return End::Error(msg!(ServerRunInternalClone)),
        }
    }

    let job_dir = base_dir.join(format!("job-{}", mode.id()));
    let mut job = agent_run::job_json(&ws, &run, mode, &branch, followup.as_ref().map(|f| &f.0));
    if let Some((_, redacted)) = &followup {
        job["followup"] = redacted.clone();
    }
    if multi {
        let guides: HashMap<String, &'static str> = targets
            .iter()
            .filter_map(|t| agent_git::guideline_file(&t.clone).map(|f| (t.repo.clone(), f)))
            .collect();
        job["repos"] = agent_run::repos_json(&run, &guides);
    } else if let Some(file) = targets
        .first()
        .and_then(|t| agent_git::guideline_file(&t.clone))
    {
        job["contributing"] = file.into();
    }
    // The diff and the description come from third parties: whatever looks like one of our keys goes.
    if let Some(review) = job.get("review").filter(|v| !v.is_null()) {
        let text = agent_git::redact(&review.to_string(), &secret_refs);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            job["review"] = v;
        }
    }
    if let Err(e) = job_write(&job_dir, &job) {
        return End::Error(msg!(ServerRunJobNotWritable, error = e.to_string()));
    }

    // Start the container.
    let name = agent_run::container_name(key, now_ms());
    let args = agent_run::run_args(&RunCommand {
        name: &name,
        run: key,
        clone: &clone,
        // Plan and review runs only read: the clone is mounted `:ro`.
        clone_only_read: mode != Mode::Implement,
        job: &job_dir,
        stdin_open: true,
        provider: access.provider,
    });
    let mut cmd = Command::new(&access.cli);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    kind_env(
        &mut cmd,
        access.provider,
        access.endpoint_url.as_deref(),
        access.key.expose(),
        &access.model,
    );
    let mut kind = match cmd.spawn() {
        Ok(k) => k,
        Err(e) => {
            return End::Error(msg!(
                ServerRunContainerNotStartedHint,
                error = e.to_string()
            ));
        }
    };
    let cli = access.cli.clone();
    let mut stdin = kind.stdin.take();
    let mut stdout = BufReader::new(kind.stdout.take().expect("stdout is piped"));
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

    let runtime = access
        .cli
        .file_name()
        .map_or_else(|| "container".into(), |n| n.to_string_lossy().into_owned());
    let _ = apply(
        state,
        key,
        AgentReport::Started {
            // The history of the run takes plain text (no codes there yet).
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
            )
            .de(),
        },
    )
    .await;

    // Time limit of the run (run time of the agent, without pauses and waiting time for you).
    let allowed_s = state
        .inner
        .settings
        .run_seconds
        .unwrap_or_else(|| allowed_seconds(ws.limits.run_minutes, run.extra_minutes));
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
            read = read_line(&mut stdout, &mut buffer) => {
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
                    RunControl::Pause => { cli_still(&cli, &["pause", &name]).await; lock(info).paused = true; }
                    RunControl::Resume => { cli_still(&cli, &["unpause", &name]).await; lock(info).paused = false; }
                    RunControl::Answer(t) => {
                        if let Some(s) = stdin.as_mut() { let _ = s.write_all(command_line("answer", &t).as_bytes()).await; let _ = s.flush().await; }
                        lock(info).question_open = false;
                    }
                    RunControl::Hint(t) => {
                        if let Some(s) = stdin.as_mut() { let _ = s.write_all(command_line("hint", &t).as_bytes()).await; let _ = s.flush().await; }
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
    drop(stdin);

    if quiet || too_much.is_some() {
        container_remove(&cli, &name).await;
        let _ = tokio::time::timeout(Duration::from_secs(10), kind.wait()).await;
        return match too_much {
            Some(m) => End::Error(m),
            None => End::Quiet,
        };
    }

    // The stream has ended: how did the container end?
    let status = match tokio::time::timeout(Duration::from_secs(30), kind.wait()).await {
        Ok(s) => s.ok(),
        Err(_) => {
            container_remove(&cli, &name).await;
            let _ = kind.wait().await;
            None
        }
    };
    let code = status.and_then(|s| s.code());
    let still_end = || {
        lock(&stderr_end)
            .iter()
            .rev()
            .map(|z| z.trim())
            .find(|z| !z.is_empty())
            .and_then(|z| last_line(z, &secret_refs))
    };
    if mode == Mode::Plan && plan_reported {
        return End::Quiet;
    }
    let Some((text, result_branch)) = result else {
        // The text of the container comes from outside: it has no code.
        let reason = error_text
            .map(|t| Msg::raw(agent_git::redact(&t, &secret_refs)))
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
            });
        return End::Error(reason);
    };
    if mode == Mode::Plan {
        return End::Error(msg!(ServerRunPlanGotResult));
    }
    if mode == Mode::Review {
        // Nothing to push: the drafts are already booked on the review.
        return End::Done(text);
    }
    if !multi && result_branch != branch {
        return End::Error(msg!(
            ServerRunBranchMismatch,
            got = result_branch,
            expected = &branch
        ));
    }

    // Check and push – the backend, not the container.
    if !multi {
        let t = targets.remove(0);
        let (k, e, b, sha, u, tk, rules) = (
            clone.clone(),
            t.export.clone(),
            branch.clone(),
            t.base_sha.clone(),
            t.url.clone(),
            access.github.clone(),
            t.rules.clone(),
        );
        let (pt, base, existing) = (
            t.pattern.clone(),
            run.options.base_branch.clone(),
            run.kind.review().is_some(),
        );
        let published = tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&e);
            let rule = agent_git::BranchRule {
                branch: &b,
                pattern: &pt,
                base: &base,
                existing,
            };
            agent_git::publish(&k, &e, &sha, &u, Some(&tk), &rules, &rule)
        })
        .await;
        return match published {
            Ok(Ok(v)) => {
                // The result per repo is booked for an implementation (a follow-up has none).
                if run.kind == RunKind::Implement {
                    let one = cockpit_core::RepoResult {
                        repo: t.repo.clone(),
                        state: cockpit_core::RunRepoState::Pushed,
                        files: v.files.clone(),
                        reason: String::new(),
                    };
                    let _ = apply(state, key, AgentReport::Repos { repos: vec![one] }).await;
                }
                // The summary of the run takes plain text (no codes there yet).
                End::Done(
                    msg!(
                        ServerRunDonePushed,
                        text = text,
                        branch = &branch,
                        n = v.files.len()
                    )
                    .de(),
                )
            }
            Ok(Err(m)) => End::Error(m),
            Err(_) => End::Error(msg!(ServerRunInternalPush)),
        };
    }
    publish_repos(state, key, &run, targets, &result_repos, &text, &access).await
}

/// One repo of a run in several repos, with everything needed to clone, check and push it.
pub(super) struct Target {
    pub repo: String,
    pub branch: String,
    pub pattern: String,
    pub base: String,
    pub url: String,
    pub clone: PathBuf,
    pub base_file: PathBuf,
    pub export: PathBuf,
    pub rules: cockpit_core::RepoRules,
    pub base_sha: String,
}

/// Each repo is handled on its own: a changed repo is checked (branch pattern, path rules) and
/// pushed, an unchanged one gets nothing, and one that fails its checks is refused without
/// blocking the others. The pull requests are created later, on your click, per repo.
async fn publish_repos(
    state: &AppState,
    key: &str,
    run: &cockpit_core::AgentRun,
    targets: Vec<Target>,
    reported: &[agent_run::ResultRepo],
    text: &str,
    access: &super::Access,
) -> End {
    use cockpit_core::{RepoResult, RunRepoState};
    let mut results: Vec<RepoResult> = Vec::new();
    let mut lines: Vec<Msg> = Vec::new();
    for t in targets {
        // The runner names the branch it committed on; a different one is not pushed.
        let wrong = reported
            .iter()
            .find(|r| r.repo == t.repo)
            .filter(|r| r.branch != t.branch)
            .map(|r| {
                msg!(
                    ServerRunBranchMismatch,
                    got = r.branch.clone(),
                    expected = &t.branch
                )
            });
        let outcome = match wrong {
            Some(m) => Err(m),
            None => {
                let (k, e, b, sha, u, tk, rules, pt, base) = (
                    t.clone.clone(),
                    t.export.clone(),
                    t.branch.clone(),
                    t.base_sha.clone(),
                    t.url.clone(),
                    access.github.clone(),
                    t.rules.clone(),
                    t.pattern.clone(),
                    t.base.clone(),
                );
                match tokio::task::spawn_blocking(move || {
                    let _ = std::fs::remove_dir_all(&e);
                    let rule = agent_git::BranchRule {
                        branch: &b,
                        pattern: &pt,
                        base: &base,
                        existing: false,
                    };
                    agent_git::publish_changes(&k, &e, &sha, &u, Some(&tk), &rules, &rule)
                })
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(msg!(ServerRunInternalPush)),
                }
            }
        };
        let (state_of, files, reason, line) = match outcome {
            Ok(agent_git::Outcome::Pushed(v)) => (
                RunRepoState::Pushed,
                v.files.clone(),
                String::new(),
                msg!(
                    ServerRunRepoPushed,
                    repo = &t.repo,
                    branch = &t.branch,
                    n = v.files.len()
                ),
            ),
            Ok(agent_git::Outcome::Unchanged) => (
                RunRepoState::Unchanged,
                Vec::new(),
                String::new(),
                msg!(ServerRunRepoUnchanged, repo = &t.repo),
            ),
            Err(m) => (
                RunRepoState::Refused,
                Vec::new(),
                m.de(),
                msg!(ServerRunRepoRefused, repo = &t.repo, reason = m),
            ),
        };
        results.push(RepoResult {
            repo: t.repo.clone(),
            state: state_of,
            files,
            reason,
        });
        lines.push(line);
    }
    match cockpit_core::judge(&results) {
        cockpit_core::Judgement::Done { .. } => {
            let _ = apply(state, key, AgentReport::Repos { repos: results }).await;
            let mut summary = text.to_string();
            for l in &lines {
                summary.push('\n');
                summary.push_str(&l.de());
            }
            let _ = run;
            End::Done(summary)
        }
        cockpit_core::Judgement::NothingChanged => End::Error(msg!(ServerRunNothingChanged)),
        cockpit_core::Judgement::AllRefused { reason } => {
            End::Error(msg!(ServerRunAllReposRefused, reason = Msg::raw(reason)))
        }
    }
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
