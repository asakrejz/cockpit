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
            tracing::warn!("Run {key} ends with error: {reason}");
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
    } else if !halted {
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
            return End::Error(format!(
                "Der Agent kann nicht starten. {}",
                missing.join(" ")
            ));
        }
        Err(_) => return End::Error("Interner Fehler beim Vorbereiten des Laufs.".into()),
    };
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
    let clone = base_dir.join(if mode == Mode::Plan { "plan" } else { "run" });
    let base_file = base_dir.join("base");
    let url = agent_git::clone_url(&access.git_base, &access.repo);

    // Clone: fresh, unless a halted run is resumed.
    let present = mode == Mode::Implement && clone.join(".git").is_dir() && base_file.is_file();
    let base_sha = if present {
        std::fs::read_to_string(&base_file)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        let (k, u, t, b, br) = (
            clone.clone(),
            url.clone(),
            access.github.clone(),
            run.options.base_branch.clone(),
            branch.clone(),
        );
        let on_existing_branch = run.kind.review().is_some();
        let bd = base_file.clone();
        let cloned = tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&k);
            if let Some(p) = k.parent() {
                std::fs::create_dir_all(p).map_err(|e| format!("Ordner nicht anlegbar: {e}"))?;
            }
            // A follow-up job continues on the branch of the pull request; a new run branches off.
            let sha = if on_existing_branch {
                agent_git::clone_branch(&u, &k, Some(&t), &br)?
            } else {
                agent_git::clone(&u, &k, Some(&t), &b, &br)?
            };
            std::fs::write(&bd, &sha).map_err(|e| format!("Stand nicht gesichert: {e}"))?;
            Ok::<_, String>(sha)
        })
        .await;
        match cloned {
            Ok(Ok(sha)) => sha,
            Ok(Err(m)) => {
                return End::Error(format!(
                    "Das Klonen von {} ist fehlgeschlagen: {}",
                    access.repo,
                    agent_git::redact(&m, &secret_refs)
                ));
            }
            Err(_) => return End::Error("Interner Fehler beim Klonen.".into()),
        }
    };

    let job_dir = base_dir.join(format!("job-{}", mode.id()));
    let mut job = agent_run::job_json(&ws, &run, mode, &branch, followup.as_ref().map(|f| &f.0));
    if let Some((_, redacted)) = &followup {
        job["followup"] = redacted.clone();
    }
    if let Err(e) = job_write(&job_dir, &job) {
        return End::Error(format!("Der Auftrag lässt sich nicht schreiben: {e}"));
    }

    // Start the container.
    let name = agent_run::container_name(key, now_ms());
    let args = agent_run::run_args(&RunCommand {
        name: &name,
        run: key,
        clone: &clone,
        clone_only_read: mode == Mode::Plan,
        job: &job_dir,
        stdin_open: true,
    });
    let mut cmd = Command::new(&access.cli);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    kind_env(
        &mut cmd,
        access.endpoint_url.as_deref(),
        access.key.expose(),
        &access.model,
    );
    let mut kind = match cmd.spawn() {
        Ok(k) => k,
        Err(e) => {
            return End::Error(format!(
                "Der Container lässt sich nicht starten: {e}. Läuft die Container-Laufzeit?"
            ));
        }
    };
    let cli = access.cli.clone();
    let mut stdin = kind.stdin.take();
    let mut stdout = BufReader::new(kind.stdout.take().expect("stdout ist umgeleitet"));
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
            detail: format!(
                "Container gestartet ({runtime}, {}), Modell {}.",
                match (mode, &run.kind) {
                    (Mode::Plan, _) => "Plan erarbeiten",
                    (Mode::Implement, RunKind::Followup { task, .. }) => match task {
                        FollowupTask::ReviewThreads => "Review-Kommentare umsetzen",
                        FollowupTask::FailedChecks { .. } => "Rote Checks beheben",
                    },
                    (Mode::Implement, _) => "Ticket umsetzen",
                },
                access.model
            ),
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
    let mut plan_reported = false;
    let mut quiet = false;
    let mut too_much: Option<String> = None;

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
                    Event::Result { text, branch: b, .. } => { result = Some((text.clone(), b.clone())); }
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
                    too_much = Some(format!("Das Zeitlimit ist überschritten: Der Container lief länger als {} Minuten und wurde beendet.", allowed_s / 60));
                    break;
                }
                if still_s > QUIET_LIMIT_S {
                    too_much = Some(format!("Der Container hat sich seit {} Minuten nicht gemeldet und wurde beendet.", QUIET_LIMIT_S / 60));
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
        let reason = error_text.map(|t| agent_git::redact(&t, &secret_refs)).unwrap_or_else(|| match code {
            Some(137) => "Der Container wurde beendet (Code 137) – meist, weil der Arbeitsspeicher der Container-Maschine nicht reicht.".to_string(),
            Some(125..=127) => format!("Der Container ließ sich nicht starten (Code {}).{}", code.unwrap_or_default(), still_end().map(|z| format!(" Meldung: {z}")).unwrap_or_default()),
            Some(0) => "Der Container endete, ohne ein Ergebnis zu melden.".to_string(),
            Some(c) => format!("Der Container endete mit Fehlercode {c}.{}", still_end().map(|z| format!(" Letzte Meldung: {z}")).unwrap_or_default()),
            None => "Der Container wurde beendet, ohne ein Ergebnis zu melden.".to_string(),
        });
        return End::Error(reason);
    };
    if mode == Mode::Plan {
        return End::Error("Der Container hat statt eines Plans ein Ergebnis gemeldet.".into());
    }
    if result_branch != branch {
        return End::Error(format!(
            "Der Container meldet den Branch „{result_branch}“, erwartet war „{branch}“. Es wird nichts gepusht."
        ));
    }

    // Check and push – the backend, not the container.
    let export = base_dir.join("export.git");
    let (k, e, b, sha, u, t, rules) = (
        clone.clone(),
        export.clone(),
        branch.clone(),
        base_sha,
        url,
        access.github.clone(),
        run.rules.clone(),
    );
    let published = tokio::task::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&e);
        agent_git::publish(&k, &e, &b, &sha, &u, Some(&t), &rules)
    })
    .await;
    match published {
        Ok(Ok(v)) => End::Done(format!(
            "{text} Branch {branch} ist gepusht ({} Dateien).",
            v.files.len()
        )),
        Ok(Err(m)) => End::Error(agent_git::redact(&m, &secret_refs)),
        Err(_) => End::Error("Interner Fehler beim Pushen.".into()),
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
