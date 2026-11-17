use super::*;

impl AppState {
    // ---------- Build image ----------

    /// The state of "Image bauen". Blocking.
    pub(crate) fn image_state(&self) -> ImageBuild {
        let mut b = lock(&self.inner.agent.build).clone();
        let status = self.container_status();
        let probe = self
            .cli_path(&status)
            .filter(|_| status.engine_running)
            .map(|cli| self.image_probe(&cli));
        b.state = probe.as_ref().map_or(ImageState::Missing, |p| p.state);
        b.present = b.state != ImageState::Missing;
        b
    }

    /// Starts the build in the background. `Err` with the explanation if it cannot start.
    /// Blockierend.
    pub(crate) fn image_build(&self) -> Result<ImageBuild, String> {
        let status = self.container_status();
        let Some(cli) = self.cli_path(&status) else {
            return Err(status.hint.unwrap_or_else(|| {
                "Es gibt keine Container-Laufzeit. Installiere Rancher Desktop oder Podman.".into()
            }));
        };
        if !status.engine_running {
            return Err(status.hint.unwrap_or_else(|| {
                "Die Container-Laufzeit läuft nicht. Starte sie und versuche es erneut.".into()
            }));
        }
        let source = self.agent_source();
        if !source.join("Containerfile").is_file() {
            return Err(format!(
                "Der Ordner mit dem Containerfile fehlt: {}. Starte Cockpit aus dem Repo-Ordner oder setze COCKPIT_AGENT_DIR.",
                source.display()
            ));
        }
        let fingerprint = agent_image::fingerprint(&source)?;
        {
            let mut b = lock(&self.inner.agent.build);
            if b.running {
                return Ok(b.clone());
            }
            *b = ImageBuild {
                running: true,
                present: false,
                state: ImageState::Missing,
                error: None,
                lines: vec!["Der Bau startet …".into()],
            };
        }
        let s = self.clone();
        tokio::spawn(async move { build_task(s, cli, source, fingerprint).await });
        Ok(lock(&self.inner.agent.build).clone())
    }
}

// ---------- Build image ----------

/// Advice for the build error, from the last lines.
pub fn build_hint(lines: &[String]) -> Option<&'static str> {
    let everything = lines.join("\n").to_lowercase();
    let contains = |s: &str| everything.contains(s);
    if contains("no space left") {
        Some(
            "Auf der Container-Maschine ist kein Platz mehr. Gib in Rancher Desktop bzw. mit „podman system prune“ Platz frei.",
        )
    } else if contains("lookup ")
        || contains("no such host")
        || contains("temporary failure in name resolution")
        || contains("connection refused")
        || contains("timeout")
        || contains("tls handshake")
    {
        Some(
            "Der Bau kommt nicht ins Netz (Pakete laden). Prüfe die Internetverbindung, ein VPN oder einen Proxy in Rancher Desktop.",
        )
    } else if contains("permission denied") && contains("docker.sock") {
        Some(
            "Der Zugriff auf Docker ist nicht erlaubt. Starte Rancher Desktop (Engine „dockerd (moby)“) neu.",
        )
    } else if contains("unauthorized") || contains("toomanyrequests") {
        Some(
            "Das Basis-Image lässt sich nicht laden (Anmeldung oder Abruflimit bei docker.io). Versuche es später erneut.",
        )
    } else {
        None
    }
}

/// The arguments of `<cli> build`: the fixed image name and the fingerprint label.
pub(super) fn build_args(containerfile: &Path, source: &Path, fingerprint: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["build".into(), "-f".into(), containerfile.into()];
    a.extend([
        "--label".into(),
        format!("{}={fingerprint}", agent_image::FINGERPRINT_LABEL).into(),
        "-t".into(),
        IMAGE.into(),
        source.into(),
    ]);
    a
}

pub(super) async fn build_task(
    state: AppState,
    cli: PathBuf,
    source: PathBuf,
    fingerprint: String,
) {
    let containerfile = source.join("Containerfile");
    let mut cmd = Command::new(&cli);
    cmd.args(build_args(&containerfile, &source, &fingerprint))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let kind = cmd.spawn();
    let done = |error: Option<String>| {
        state.image_forget();
        let mut b = lock(&state.inner.agent.build);
        b.running = false;
        b.present = error.is_none();
        b.state = if error.is_none() {
            ImageState::Current
        } else {
            ImageState::Missing
        };
        b.error = error;
    };
    let mut kind = match kind {
        Ok(k) => k,
        Err(e) => {
            done(Some(format!("Der Bau lässt sich nicht starten: {e}")));
            return;
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    for (strom, tx) in [
        (
            kind.stdout.take().map(BufReader::new).map(Strom::Off),
            tx.clone(),
        ),
        (
            kind.stderr.take().map(BufReader::new).map(Strom::Error),
            tx.clone(),
        ),
    ] {
        if let Some(strom) = strom {
            tokio::spawn(async move { strom.lines(tx).await });
        }
    }
    drop(tx);
    while let Some(z) = rx.recv().await {
        let z = z.trim_end().to_string();
        if z.is_empty() {
            continue;
        }
        let mut b = lock(&state.inner.agent.build);
        b.lines.push(z);
        let n = b.lines.len();
        if n > MAX_BUILD_LINES {
            b.lines.drain(..n - MAX_BUILD_LINES);
        }
    }
    let status = kind.wait().await;
    match status {
        Ok(s) if s.success() => done(None),
        Ok(s) => {
            let lines = lock(&state.inner.agent.build).lines.clone();
            let last = lines.iter().rev().find(|z| !z.trim().is_empty()).cloned();
            let mut m = format!(
                "Der Bau ist gescheitert (Code {}).",
                s.code().map_or("-".into(), |c| c.to_string())
            );
            if let Some(h) = build_hint(&lines) {
                m.push(' ');
                m.push_str(h);
            }
            if let Some(l) = last {
                m.push_str(&format!(" Letzte Meldung: {l}"));
            }
            done(Some(m));
        }
        Err(e) => done(Some(format!("Der Bau wurde unterbrochen: {e}"))),
    }
}

pub(super) enum Strom {
    Off(BufReader<tokio::process::ChildStdout>),
    Error(BufReader<tokio::process::ChildStderr>),
}

impl Strom {
    async fn lines(self, tx: mpsc::UnboundedSender<String>) {
        async fn read<R: AsyncBufRead + Unpin>(mut r: R, tx: mpsc::UnboundedSender<String>) {
            let mut buffer = Vec::new();
            while let Ok(Some(g)) = read_line(&mut r, &mut buffer).await {
                let t = match g {
                    Read::Line(z) => z,
                    Read::TooLarge => "(sehr lange Zeile ausgelassen)".into(),
                };
                if tx.send(t).is_err() {
                    break;
                }
            }
        }
        match self {
            Strom::Off(r) => read(r, tx).await,
            Strom::Error(r) => read(r, tx).await,
        }
    }
}
