use super::*;

impl AppState {
    // ---------- Image bauen ----------

    /// Der Stand von „Image bauen“. Blockierend.
    pub(crate) fn image_stand(&self) -> ImageBuild {
        let mut b = sperre(&self.inner.agent.bau).clone();
        let status = self.container_status();
        b.present = status.engine_running
            && self
                .cli_pfad(&status)
                .is_some_and(|cli| self.image_vorhanden(&cli));
        b
    }

    /// Startet den Bau im Hintergrund. `Err` mit der Erklärung, wenn er nicht starten kann.
    /// Blockierend.
    pub(crate) fn image_bauen(&self) -> Result<ImageBuild, String> {
        let status = self.container_status();
        let Some(cli) = self.cli_pfad(&status) else {
            return Err(status.hint.unwrap_or_else(|| {
                "Es gibt keine Container-Laufzeit. Installiere Rancher Desktop oder Podman.".into()
            }));
        };
        if !status.engine_running {
            return Err(status.hint.unwrap_or_else(|| {
                "Die Container-Laufzeit läuft nicht. Starte sie und versuche es erneut.".into()
            }));
        }
        let quelle = self.agent_quelle();
        if !quelle.join("Containerfile").is_file() {
            return Err(format!(
                "Der Ordner mit dem Containerfile fehlt: {}. Starte Cockpit aus dem Repo-Ordner oder setze COCKPIT_AGENT_DIR.",
                quelle.display()
            ));
        }
        {
            let mut b = sperre(&self.inner.agent.bau);
            if b.running {
                return Ok(b.clone());
            }
            *b = ImageBuild {
                running: true,
                present: false,
                error: None,
                lines: vec!["Der Bau startet …".into()],
            };
        }
        let s = self.clone();
        tokio::spawn(async move { bau_aufgabe(s, cli, quelle).await });
        Ok(sperre(&self.inner.agent.bau).clone())
    }
}

// ---------- Image bauen ----------

/// Ein Rat zum Fehler beim Bauen, aus den letzten Zeilen.
pub fn bau_hinweis(zeilen: &[String]) -> Option<&'static str> {
    let alles = zeilen.join("\n").to_lowercase();
    let enthaelt = |s: &str| alles.contains(s);
    if enthaelt("no space left") {
        Some(
            "Auf der Container-Maschine ist kein Platz mehr. Gib in Rancher Desktop bzw. mit „podman system prune“ Platz frei.",
        )
    } else if enthaelt("lookup ")
        || enthaelt("no such host")
        || enthaelt("temporary failure in name resolution")
        || enthaelt("connection refused")
        || enthaelt("timeout")
        || enthaelt("tls handshake")
    {
        Some(
            "Der Bau kommt nicht ins Netz (Pakete laden). Prüfe die Internetverbindung, ein VPN oder einen Proxy in Rancher Desktop.",
        )
    } else if enthaelt("permission denied") && enthaelt("docker.sock") {
        Some(
            "Der Zugriff auf Docker ist nicht erlaubt. Starte Rancher Desktop (Engine „dockerd (moby)“) neu.",
        )
    } else if enthaelt("unauthorized") || enthaelt("toomanyrequests") {
        Some(
            "Das Basis-Image lässt sich nicht laden (Anmeldung oder Abruflimit bei docker.io). Versuche es später erneut.",
        )
    } else {
        None
    }
}

pub(super) async fn bau_aufgabe(state: AppState, cli: PathBuf, quelle: PathBuf) {
    let containerfile = quelle.join("Containerfile");
    let mut cmd = Command::new(&cli);
    cmd.args(["build", "-f"])
        .arg(&containerfile)
        .args(["-t", IMAGE])
        .arg(&quelle)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let kind = cmd.spawn();
    let fertig = |fehler: Option<String>| {
        state.image_vergessen();
        let mut b = sperre(&state.inner.agent.bau);
        b.running = false;
        b.present = fehler.is_none();
        b.error = fehler;
    };
    let mut kind = match kind {
        Ok(k) => k,
        Err(e) => {
            fertig(Some(format!("Der Bau lässt sich nicht starten: {e}")));
            return;
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    for (strom, tx) in [
        (
            kind.stdout.take().map(BufReader::new).map(Strom::Aus),
            tx.clone(),
        ),
        (
            kind.stderr.take().map(BufReader::new).map(Strom::Fehler),
            tx.clone(),
        ),
    ] {
        if let Some(strom) = strom {
            tokio::spawn(async move { strom.zeilen(tx).await });
        }
    }
    drop(tx);
    while let Some(z) = rx.recv().await {
        let z = z.trim_end().to_string();
        if z.is_empty() {
            continue;
        }
        let mut b = sperre(&state.inner.agent.bau);
        b.lines.push(z);
        let n = b.lines.len();
        if n > MAX_BAU_ZEILEN {
            b.lines.drain(..n - MAX_BAU_ZEILEN);
        }
    }
    let status = kind.wait().await;
    match status {
        Ok(s) if s.success() => fertig(None),
        Ok(s) => {
            let zeilen = sperre(&state.inner.agent.bau).lines.clone();
            let letzte = zeilen.iter().rev().find(|z| !z.trim().is_empty()).cloned();
            let mut m = format!(
                "Der Bau ist gescheitert (Code {}).",
                s.code().map_or("-".into(), |c| c.to_string())
            );
            if let Some(h) = bau_hinweis(&zeilen) {
                m.push(' ');
                m.push_str(h);
            }
            if let Some(l) = letzte {
                m.push_str(&format!(" Letzte Meldung: {l}"));
            }
            fertig(Some(m));
        }
        Err(e) => fertig(Some(format!("Der Bau wurde unterbrochen: {e}"))),
    }
}

pub(super) enum Strom {
    Aus(BufReader<tokio::process::ChildStdout>),
    Fehler(BufReader<tokio::process::ChildStderr>),
}

impl Strom {
    async fn zeilen(self, tx: mpsc::UnboundedSender<String>) {
        async fn lies<R: AsyncBufRead + Unpin>(mut r: R, tx: mpsc::UnboundedSender<String>) {
            let mut puffer = Vec::new();
            while let Ok(Some(g)) = lies_zeile(&mut r, &mut puffer).await {
                let t = match g {
                    Gelesen::Zeile(z) => z,
                    Gelesen::ZuGross => "(sehr lange Zeile ausgelassen)".into(),
                };
                if tx.send(t).is_err() {
                    break;
                }
            }
        }
        match self {
            Strom::Aus(r) => lies(r, tx).await,
            Strom::Fehler(r) => lies(r, tx).await,
        }
    }
}
