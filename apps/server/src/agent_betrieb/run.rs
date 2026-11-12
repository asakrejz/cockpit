use super::*;

// ---------- Ein Lauf ----------

pub(super) fn auftrag_schreiben(ordner: &Path, inhalt: &serde_json::Value) -> std::io::Result<()> {
    std::fs::create_dir_all(ordner)?;
    let datei = ordner.join("auftrag.json");
    std::fs::write(&datei, inhalt.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(ordner, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&datei, std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

pub(super) async fn lauf_aufgabe(
    state: AppState,
    key: String,
    modus: Modus,
    mut rx: mpsc::UnboundedReceiver<Befehl>,
    info: Arc<Mutex<Info>>,
) {
    let ende = lauf_ausfuehren(&state, &key, modus, &mut rx, &info).await;
    match ende {
        Ende::Ruhig => {}
        Ende::Fehler(grund) => {
            tracing::warn!("Lauf {key} endet mit Fehler: {grund}");
            sperre(&state.inner.agent.gescheitert).insert(key.clone(), Instant::now());
            if let Err(e) = state
                .melden(&key, AgentReport::Failed { reason: grund })
                .await
            {
                tracing::debug!("Fehler des Laufs {key} nicht mehr zu melden: {e:?}");
            }
        }
        Ende::Fertig(summary) => {
            if let Err(e) = state.melden(&key, AgentReport::Done { summary }).await {
                tracing::debug!("Ergebnis des Laufs {key} nicht mehr zu melden: {e:?}");
            }
        }
    }
    // Aufräumen: Der Klon bleibt nur, solange der Lauf von einer Grenze angehalten wartet.
    let angehalten = state
        .inner
        .live
        .kopie()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_stopped());
    let ordner = state
        .agent_ordner()
        .join("laeufe")
        .join(lauf::lauf_ordner(&key));
    if modus == Modus::Plan {
        let _ = tokio::fs::remove_dir_all(ordner.join("plan")).await;
    } else if !angehalten {
        let _ = tokio::fs::remove_dir_all(&ordner).await;
    }
    sperre(&state.inner.agent.laufend).remove(&key);
}

pub(super) enum Anwendung {
    Ok,
    /// Der Lauf ist nicht mehr aktiv oder die Meldung passt nicht (mehr).
    Abgewiesen,
    Fehler,
}

pub(super) async fn anwenden(state: &AppState, key: &str, r: AgentReport) -> Anwendung {
    match state.melden(key, r).await {
        Ok(()) => Anwendung::Ok,
        Err(Fehler::Aktion(ActionError::NothingToDo)) => Anwendung::Abgewiesen,
        Err(Fehler::Aktion(_)) => Anwendung::Abgewiesen,
        Err(Fehler::Speichern(m)) => {
            tracing::warn!("Meldung nicht gesichert: {m}");
            Anwendung::Fehler
        }
    }
}

pub(super) fn befehlszeile(t: &str, text: &str) -> String {
    format!("{}\n", serde_json::json!({ "t": t, "text": text }))
}

#[allow(clippy::too_many_lines)]
pub(super) async fn lauf_ausfuehren(
    state: &AppState,
    key: &str,
    modus: Modus,
    rx: &mut mpsc::UnboundedReceiver<Befehl>,
    info: &Arc<Mutex<Info>>,
) -> Ende {
    let ws: Workspace = state.inner.live.kopie();
    let Some(run) = ws.run(key) else {
        return Ende::Ruhig;
    };
    state.oauth_auffrischen().await;
    let s = state.clone();
    let zugang = match tokio::task::spawn_blocking(move || s.zugang()).await {
        Ok(Ok(z)) => z,
        Ok(Err(fehlt)) => {
            return Ende::Fehler(format!("Der Agent kann nicht starten. {}", fehlt.join(" ")));
        }
        Err(_) => return Ende::Fehler("Interner Fehler beim Vorbereiten des Laufs.".into()),
    };
    let geheim = [
        zugang.key.expose().to_string(),
        zugang.github.expose().to_string(),
    ];
    let geheim_refs: Vec<&str> = geheim.iter().map(String::as_str).collect();

    let branch = run.branch();
    let basis_ordner = state
        .agent_ordner()
        .join("laeufe")
        .join(lauf::lauf_ordner(key));
    let klon = basis_ordner.join(if modus == Modus::Plan { "plan" } else { "lauf" });
    let basis_datei = basis_ordner.join("basis");
    let url = agent_git::klon_url(&zugang.git_basis, &zugang.repo);

    // Klon: frisch, außer ein angehaltener Lauf wird fortgesetzt.
    let vorhanden = modus == Modus::Umsetzen && klon.join(".git").is_dir() && basis_datei.is_file();
    let basis_sha = if vorhanden {
        std::fs::read_to_string(&basis_datei)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        let (k, u, t, b, br) = (
            klon.clone(),
            url.clone(),
            zugang.github.clone(),
            run.options.base_branch.clone(),
            branch.clone(),
        );
        let bd = basis_datei.clone();
        let geklont = tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&k);
            if let Some(p) = k.parent() {
                std::fs::create_dir_all(p).map_err(|e| format!("Ordner nicht anlegbar: {e}"))?;
            }
            let sha = agent_git::klonen(&u, &k, Some(&t), &b, &br)?;
            std::fs::write(&bd, &sha).map_err(|e| format!("Stand nicht gesichert: {e}"))?;
            Ok::<_, String>(sha)
        })
        .await;
        match geklont {
            Ok(Ok(sha)) => sha,
            Ok(Err(m)) => {
                return Ende::Fehler(format!(
                    "Das Klonen von {} ist fehlgeschlagen: {}",
                    zugang.repo,
                    agent_git::schwaerzen(&m, &geheim_refs)
                ));
            }
            Err(_) => return Ende::Fehler("Interner Fehler beim Klonen.".into()),
        }
    };

    let auftrag_ordner = basis_ordner.join(format!("auftrag-{}", modus.kennung()));
    if let Err(e) = auftrag_schreiben(
        &auftrag_ordner,
        &lauf::auftrag_json(&ws, &run, modus, &branch),
    ) {
        return Ende::Fehler(format!("Der Auftrag lässt sich nicht schreiben: {e}"));
    }

    // Container starten.
    let name = lauf::container_name(key, jetzt_ms());
    let args = lauf::run_args(&LaufBefehl {
        name: &name,
        lauf: key,
        klon: &klon,
        klon_nur_lesen: modus == Modus::Plan,
        auftrag: &auftrag_ordner,
        stdin_offen: true,
    });
    let mut cmd = Command::new(&zugang.cli);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    kind_umgebung(
        &mut cmd,
        zugang.endpunkt_url.as_deref(),
        zugang.key.expose(),
        &zugang.modell,
    );
    let mut kind = match cmd.spawn() {
        Ok(k) => k,
        Err(e) => {
            return Ende::Fehler(format!(
                "Der Container lässt sich nicht starten: {e}. Läuft die Container-Laufzeit?"
            ));
        }
    };
    let cli = zugang.cli.clone();
    let mut stdin = kind.stdin.take();
    let mut stdout = BufReader::new(kind.stdout.take().expect("stdout ist umgeleitet"));
    let stderr_ende: Arc<Mutex<VecDeque<String>>> = Arc::default();
    if let Some(se) = kind.stderr.take() {
        let ende = stderr_ende.clone();
        tokio::spawn(async move {
            let mut r = BufReader::new(se);
            let mut puffer = Vec::new();
            while let Ok(Some(Gelesen::Zeile(z))) = lies_zeile(&mut r, &mut puffer).await {
                let mut e = sperre(&ende);
                e.push_back(z);
                if e.len() > 20 {
                    e.pop_front();
                }
            }
        });
    }

    let laufzeit = zugang
        .cli
        .file_name()
        .map_or_else(|| "container".into(), |n| n.to_string_lossy().into_owned());
    let _ = anwenden(
        state,
        key,
        AgentReport::Started {
            detail: format!(
                "Container gestartet ({laufzeit}, {}), Modell {}.",
                match modus {
                    Modus::Plan => "Plan erarbeiten",
                    Modus::Umsetzen => "Ticket umsetzen",
                },
                zugang.modell
            ),
        },
    )
    .await;

    // Zeitlimit des Laufs (Laufzeit des Agenten, ohne Pausen und Wartezeit auf dich).
    let erlaubt_s = state
        .inner
        .einstellungen
        .lauf_sekunden
        .unwrap_or_else(|| erlaubte_sekunden(ws.limits.run_minutes, run.extra_minutes));
    let (mut aktiv_s, mut still_s) = (0u64, 0u64);
    let mut takt = tokio::time::interval(Duration::from_secs(1));
    let mut puffer = Vec::new();
    let mut verworfen = 0usize;
    let mut ergebnis: Option<(String, String)> = None;
    let mut fehler_text: Option<String> = None;
    let mut plan_gemeldet = false;
    let mut ruhig = false;
    let mut zu_viel: Option<String> = None;

    loop {
        tokio::select! {
            gelesen = lies_zeile(&mut stdout, &mut puffer) => {
                let zeile = match gelesen {
                    Ok(Some(Gelesen::Zeile(z))) => z,
                    Ok(Some(Gelesen::ZuGross)) => { verworfen += 1; continue; }
                    Ok(None) | Err(_) => break,
                };
                still_s = 0;
                let e = match lauf::zeile_lesen(&zeile) {
                    Ok(e) => e,
                    Err(_) => { verworfen += 1; continue; }
                };
                match &e {
                    Ereignis::Ergebnis { text, branch: b, .. } => { ergebnis = Some((text.clone(), b.clone())); }
                    Ereignis::Fehler { text } => { fehler_text = Some(text.clone()); }
                    Ereignis::Frage { .. } if modus == Modus::Umsetzen => {
                        sperre(info).frage_offen = true;
                    }
                    Ereignis::HinweiseAngekommen => {
                        let mut i = sperre(info);
                        i.hinweise_gesendet = i.hinweise_gesendet.saturating_sub(1);
                    }
                    _ => {}
                }
                if let Some(meldung) = lauf::als_meldung(&e, modus) {
                    let ist_plan = matches!(meldung, AgentReport::Plan { .. });
                    let ist_frage = matches!(meldung, AgentReport::Question { .. });
                    match anwenden(state, key, meldung).await {
                        Anwendung::Ok => { plan_gemeldet |= ist_plan; }
                        Anwendung::Abgewiesen => {
                            if ist_frage { sperre(info).frage_offen = false; }
                            // Der Lauf ist nicht mehr aktiv oder wartet nicht mehr darauf.
                            if !lauf_aktiv(state, key) { ruhig = true; break; }
                        }
                        Anwendung::Fehler => {}
                    }
                    // Eine Grenze (Budget, Zeit) kann diese Meldung ausgelöst haben.
                    if !ruhig && lauf_angehalten_oder_weg(state, key) { ruhig = true; break; }
                }
            }
            Some(b) = rx.recv() => {
                match b {
                    Befehl::Stoppen => { ruhig = true; break; }
                    Befehl::Pausieren => { cli_still(&cli, &["pause", &name]).await; sperre(info).pausiert = true; }
                    Befehl::Fortsetzen => { cli_still(&cli, &["unpause", &name]).await; sperre(info).pausiert = false; }
                    Befehl::Antwort(t) => {
                        if let Some(s) = stdin.as_mut() { let _ = s.write_all(befehlszeile("antwort", &t).as_bytes()).await; let _ = s.flush().await; }
                        sperre(info).frage_offen = false;
                    }
                    Befehl::Hinweis(t) => {
                        if let Some(s) = stdin.as_mut() { let _ = s.write_all(befehlszeile("hinweis", &t).as_bytes()).await; let _ = s.flush().await; }
                        sperre(info).hinweise_gesendet += 1;
                    }
                }
            }
            _ = takt.tick() => {
                let (pausiert, fragt) = { let i = sperre(info); (i.pausiert, i.frage_offen) };
                if !pausiert && !fragt {
                    aktiv_s += 1;
                    still_s += 1;
                }
                if aktiv_s > erlaubt_s {
                    zu_viel = Some(format!("Das Zeitlimit ist überschritten: Der Container lief länger als {} Minuten und wurde beendet.", erlaubt_s / 60));
                    break;
                }
                if still_s > STILLE_GRENZE_S {
                    zu_viel = Some(format!("Der Container hat sich seit {} Minuten nicht gemeldet und wurde beendet.", STILLE_GRENZE_S / 60));
                    break;
                }
            }
        }
    }
    if verworfen > 0 {
        tracing::warn!("Lauf {key}: {verworfen} unbrauchbare Zeilen des Containers verworfen");
    }
    drop(stdin);

    if ruhig || zu_viel.is_some() {
        container_entfernen(&cli, &name).await;
        let _ = tokio::time::timeout(Duration::from_secs(10), kind.wait()).await;
        return match zu_viel {
            Some(m) => Ende::Fehler(m),
            None => Ende::Ruhig,
        };
    }

    // Der Strom ist zu Ende: Wie hat der Container geendet?
    let status = match tokio::time::timeout(Duration::from_secs(30), kind.wait()).await {
        Ok(s) => s.ok(),
        Err(_) => {
            container_entfernen(&cli, &name).await;
            let _ = kind.wait().await;
            None
        }
    };
    let code = status.and_then(|s| s.code());
    let still_ende = || {
        sperre(&stderr_ende)
            .iter()
            .rev()
            .map(|z| z.trim())
            .find(|z| !z.is_empty())
            .and_then(|z| letzte_zeile(z, &geheim_refs))
    };
    if modus == Modus::Plan && plan_gemeldet {
        return Ende::Ruhig;
    }
    let Some((text, ergebnis_branch)) = ergebnis else {
        let grund = fehler_text.map(|t| agent_git::schwaerzen(&t, &geheim_refs)).unwrap_or_else(|| match code {
            Some(137) => "Der Container wurde beendet (Code 137) – meist, weil der Arbeitsspeicher der Container-Maschine nicht reicht.".to_string(),
            Some(125..=127) => format!("Der Container ließ sich nicht starten (Code {}).{}", code.unwrap_or_default(), still_ende().map(|z| format!(" Meldung: {z}")).unwrap_or_default()),
            Some(0) => "Der Container endete, ohne ein Ergebnis zu melden.".to_string(),
            Some(c) => format!("Der Container endete mit Fehlercode {c}.{}", still_ende().map(|z| format!(" Letzte Meldung: {z}")).unwrap_or_default()),
            None => "Der Container wurde beendet, ohne ein Ergebnis zu melden.".to_string(),
        });
        return Ende::Fehler(grund);
    };
    if modus == Modus::Plan {
        return Ende::Fehler("Der Container hat statt eines Plans ein Ergebnis gemeldet.".into());
    }
    if ergebnis_branch != branch {
        return Ende::Fehler(format!(
            "Der Container meldet den Branch „{ergebnis_branch}“, erwartet war „{branch}“. Es wird nichts gepusht."
        ));
    }

    // Prüfen und pushen – das Backend, nicht der Container.
    let export = basis_ordner.join("export.git");
    let (k, e, b, sha, u, t, regeln) = (
        klon.clone(),
        export.clone(),
        branch.clone(),
        basis_sha,
        url,
        zugang.github.clone(),
        run.rules.clone(),
    );
    let veroeffentlicht = tokio::task::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&e);
        agent_git::veroeffentlichen(&k, &e, &b, &sha, &u, Some(&t), &regeln)
    })
    .await;
    match veroeffentlicht {
        Ok(Ok(v)) => Ende::Fertig(format!(
            "{text} Branch {branch} ist gepusht ({} Dateien).",
            v.dateien.len()
        )),
        Ok(Err(m)) => Ende::Fehler(agent_git::schwaerzen(&m, &geheim_refs)),
        Err(_) => Ende::Fehler("Interner Fehler beim Pushen.".into()),
    }
}

pub(super) fn lauf_aktiv(state: &AppState, key: &str) -> bool {
    state
        .inner
        .live
        .kopie()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_active())
}

/// Der Lauf ist nicht mehr aktiv oder wurde von einer Grenze angehalten: Der Container soll enden.
pub(super) fn lauf_angehalten_oder_weg(state: &AppState, key: &str) -> bool {
    !state
        .inner
        .live
        .kopie()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_active() && !r.is_stopped())
}
