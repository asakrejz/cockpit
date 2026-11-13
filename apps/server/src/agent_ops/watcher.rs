use super::*;

impl AppState {
    // ---------- Aufpasser ----------

    /// Ein Durchgang: vergleicht Läufe und Container und führt aus, was nötig ist.
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
        for a in agent_run::plane(&ws, &infos, &blocked) {
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

    // ---------- Start und Aufräumen ----------

    /// Beim Start des Servers: Container eines früheren Starts entfernen und Läufe, die dabei
    /// verloren gingen, als abgebrochen kennzeichnen. Blockierend.
    pub(crate) fn agent_clean_up(&self) {
        let status = self.container_status();
        if let Some(cli) = self.cli_path(&status).filter(|_| status.engine_running) {
            let n = container_labels_remove(&cli);
            if n > 0 {
                tracing::info!("{n} Container eines früheren Starts entfernt");
            }
        }
        let ws = self.inner.live.copy();
        let mut keep = HashSet::new();
        for r in ws.runs.iter().filter(|r| r.kind == RunKind::Implement) {
            let key = r.key();
            match r.state {
                // Wartet nur auf dich: Plan liegt vor, oder eine Grenze hat angehalten (Klon bleibt).
                RunState::AwaitingApproval if r.agent_plan.is_some() => {}
                RunState::Stopped(_) => {
                    keep.insert(agent_run::run_dir(&key));
                }
                RunState::Running
                | RunState::AwaitingAnswer
                | RunState::Paused
                | RunState::AwaitingApproval => {
                    let reason = "Der Server wurde neu gestartet, dabei ging der Container verloren. Starte den Lauf bei Bedarf neu.";
                    if let Err(e) = self.report_blocking(
                        &key,
                        AgentReport::Failed {
                            reason: reason.into(),
                        },
                    ) {
                        tracing::warn!("Lauf {key} nicht als abgebrochen markiert: {e:?}");
                    }
                }
                _ => {}
            }
        }
        // Klone und Aufträge, die keinem wartenden Lauf gehören.
        if let Ok(entries) = std::fs::read_dir(self.agent_dir().join("runs")) {
            for e in entries.flatten() {
                if !keep.contains(e.file_name().to_string_lossy().as_ref()) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// Startet den Aufpasser und den Takt der übrigen Aufgaben (ohne simulierten Agenten).
pub fn watcher_start(state: AppState) {
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.agent_clean_up());
    tokio::spawn(async move {
        // Erst aufräumen lassen, dann beobachten.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut takt = tokio::time::interval(WATCHER_TAKT);
        let mut counter = 0u64;
        loop {
            takt.tick().await;
            counter += 1;
            state.watch().await;
            // Die übrigen Aufgaben (Checks, Zurückgestelltes, Verknüpfen) im gewohnten Takt.
            if counter.is_multiple_of(2) {
                let s = state.clone();
                let _ = tokio::task::spawn_blocking(move || s.takt()).await;
            }
        }
    });
}
