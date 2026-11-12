use super::*;

impl AppState {
    // ---------- Aufpasser ----------

    /// Ein Durchgang: vergleicht Läufe und Container und führt aus, was nötig ist.
    pub(crate) async fn aufpassen(&self) {
        let ws = self.inner.live.kopie();
        let (infos, gesperrt) = {
            let l = sperre(&self.inner.agent.laufend);
            let infos: HashMap<String, Info> = l
                .iter()
                .map(|(k, h)| (k.clone(), sperre(&h.info).clone()))
                .collect();
            let mut g = sperre(&self.inner.agent.gescheitert);
            g.retain(|_, t| t.elapsed() < SPERRE_NACH_FEHLER);
            (infos, g.keys().cloned().collect::<Vec<_>>())
        };
        for a in lauf::plane(&ws, &infos, &gesperrt) {
            match a {
                Anweisung::Starten(key, modus) => self.lauf_starten(&key, modus),
                Anweisung::Stoppen(key) => self.befehl(&key, Befehl::Stoppen),
                Anweisung::Pausieren(key) => self.befehl(&key, Befehl::Pausieren),
                Anweisung::Fortsetzen(key) => self.befehl(&key, Befehl::Fortsetzen),
                Anweisung::Antwort(key, text) => self.befehl(&key, Befehl::Antwort(text)),
                Anweisung::Hinweise(key, texte) => {
                    for t in texte {
                        self.befehl(&key, Befehl::Hinweis(t));
                    }
                }
            }
        }
    }

    pub(super) fn befehl(&self, key: &str, b: Befehl) {
        if let Some(h) = sperre(&self.inner.agent.laufend).get(key) {
            let _ = h.tx.send(b);
        }
    }

    pub(super) fn lauf_starten(&self, key: &str, modus: Modus) {
        let (tx, rx) = mpsc::unbounded_channel();
        let info = Arc::new(Mutex::new(Info {
            modus,
            pausiert: false,
            frage_offen: false,
            hinweise_gesendet: 0,
        }));
        {
            let mut l = sperre(&self.inner.agent.laufend);
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
        tokio::spawn(async move { lauf_aufgabe(s, k, modus, rx, info).await });
    }

    // ---------- Start und Aufräumen ----------

    /// Beim Start des Servers: Container eines früheren Starts entfernen und Läufe, die dabei
    /// verloren gingen, als abgebrochen kennzeichnen. Blockierend.
    pub(crate) fn agent_aufraeumen(&self) {
        let status = self.container_status();
        if let Some(cli) = self.cli_pfad(&status).filter(|_| status.engine_running) {
            let n = container_labels_entfernen(&cli);
            if n > 0 {
                tracing::info!("{n} Container eines früheren Starts entfernt");
            }
        }
        let ws = self.inner.live.kopie();
        let mut behalten = HashSet::new();
        for r in ws.runs.iter().filter(|r| r.kind == RunKind::Implement) {
            let key = r.key();
            match r.state {
                // Wartet nur auf dich: Plan liegt vor, oder eine Grenze hat angehalten (Klon bleibt).
                RunState::AwaitingApproval if r.agent_plan.is_some() => {}
                RunState::Stopped(_) => {
                    behalten.insert(lauf::lauf_ordner(&key));
                }
                RunState::Running
                | RunState::AwaitingAnswer
                | RunState::Paused
                | RunState::AwaitingApproval => {
                    let grund = "Der Server wurde neu gestartet, dabei ging der Container verloren. Starte den Lauf bei Bedarf neu.";
                    if let Err(e) = self.melden_blockierend(
                        &key,
                        AgentReport::Failed {
                            reason: grund.into(),
                        },
                    ) {
                        tracing::warn!("Lauf {key} nicht als abgebrochen markiert: {e:?}");
                    }
                }
                _ => {}
            }
        }
        // Klone und Aufträge, die keinem wartenden Lauf gehören.
        if let Ok(eintraege) = std::fs::read_dir(self.agent_ordner().join("laeufe")) {
            for e in eintraege.flatten() {
                if !behalten.contains(e.file_name().to_string_lossy().as_ref()) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// Startet den Aufpasser und den Takt der übrigen Aufgaben (ohne simulierten Agenten).
pub fn aufpasser_starten(state: AppState) {
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.agent_aufraeumen());
    tokio::spawn(async move {
        // Erst aufräumen lassen, dann beobachten.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut takt = tokio::time::interval(AUFPASSER_TAKT);
        let mut zaehler = 0u64;
        loop {
            takt.tick().await;
            zaehler += 1;
            state.aufpassen().await;
            // Die übrigen Aufgaben (Checks, Zurückgestelltes, Verknüpfen) im gewohnten Takt.
            if zaehler.is_multiple_of(2) {
                let s = state.clone();
                let _ = tokio::task::spawn_blocking(move || s.takt()).await;
            }
        }
    });
}
