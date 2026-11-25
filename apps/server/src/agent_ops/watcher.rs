use super::*;

impl AppState {
    // ---------- Watcher ----------

    /// One pass: compares runs and containers and carries out what is necessary.
    pub(crate) async fn watch(&self) {
        let ws = self.inner.live.copy();
        let (infos, blocked) = {
            let l = lock(&self.inner.agent.running);
            let infos: HashMap<String, Info> = l
                .iter()
                .map(|(k, h)| (k.clone(), lock(&h.info).clone()))
                .collect();
            let mut g = lock(&self.inner.agent.failed);
            g.retain(|_, t| t.elapsed() < LOCK_AFTER_ERROR);
            (infos, g.keys().cloned().collect::<Vec<_>>())
        };
        for a in agent_run::plan_instructions(&ws, &infos, &blocked) {
            match a {
                Instruction::Start(key, mode) => self.run_start(&key, mode),
                Instruction::Stop(key) => self.command(&key, RunControl::Stop),
                Instruction::Pause(key) => self.command(&key, RunControl::Pause),
                Instruction::Resume(key) => self.command(&key, RunControl::Resume),
                Instruction::Answer(key, text) => self.command(&key, RunControl::Answer(text)),
                Instruction::Hints(key, texts) => {
                    for t in texts {
                        self.command(&key, RunControl::Hint(t));
                    }
                }
            }
        }
    }

    pub(super) fn command(&self, key: &str, b: RunControl) {
        if let Some(h) = lock(&self.inner.agent.running).get(key) {
            let _ = h.tx.send(b);
        }
    }

    pub(super) fn run_start(&self, key: &str, mode: Mode) {
        let (tx, rx) = mpsc::unbounded_channel();
        let info = Arc::new(Mutex::new(Info {
            mode,
            paused: false,
            question_open: false,
            hints_sent: 0,
        }));
        {
            let mut l = lock(&self.inner.agent.running);
            if l.contains_key(key) {
                return;
            }
            l.insert(
                key.to_string(),
                Handle {
                    info: info.clone(),
                    tx,
                },
            );
        }
        let s = self.clone();
        let k = key.to_string();
        tokio::spawn(async move { run_task(s, k, mode, rx, info).await });
    }

    // ---------- Start and clean-up ----------

    /// At server start: remove containers of an earlier start and mark runs that were
    /// lost in the process as cancelled. Blocking.
    pub(crate) fn agent_clean_up(&self) {
        let status = self.container_status();
        if let Some(cli) = self.cli_path(&status).filter(|_| status.engine_running) {
            let n = container_labels_remove(&cli);
            if n > 0 {
                tracing::info!("{n} containers from an earlier start removed");
            }
        }
        let ws = self.inner.live.copy();
        let mut keep = HashSet::new();
        for r in &ws.runs {
            let key = r.key();
            match r.state {
                // Only waits for you: a plan is ready, or a limit has halted (clone stays).
                RunState::AwaitingApproval if r.agent_plan.is_some() => {}
                RunState::Stopped(_) => {
                    keep.insert(agent_run::run_dir(&key));
                }
                RunState::Running
                | RunState::AwaitingAnswer
                | RunState::Paused
                | RunState::AwaitingApproval => {
                    let reason = cockpit_i18n::msg!(ServerRunRestartLost);
                    if let Err(e) = self.report_blocking(&key, AgentReport::Failed { reason }) {
                        tracing::warn!("Run {key} not marked as aborted: {e:?}");
                    }
                }
                _ => {}
            }
        }
        // Clones and jobs that do not belong to a waiting run.
        if let Ok(entries) = std::fs::read_dir(self.agent_dir().join("runs")) {
            for e in entries.flatten() {
                if !keep.contains(e.file_name().to_string_lossy().as_ref()) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// Starts the watcher of the containers and the tick of the other tasks (snoozes, linking, status).
pub fn watcher_start(state: AppState) {
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.agent_clean_up());
    tokio::spawn(async move {
        // Clean up first, then watch.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut ticker = tokio::time::interval(WATCHER_TICK);
        let mut counter = 0u64;
        loop {
            ticker.tick().await;
            counter += 1;
            state.watch().await;
            // The other tasks (checks, snoozed items, linking) in the usual tick.
            if counter.is_multiple_of(2) {
                let s = state.clone();
                let _ = tokio::task::spawn_blocking(move || s.tick()).await;
            }
        }
    });
}
