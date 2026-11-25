use cockpit_i18n::{ArgValue, Msg, msg};

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
    /// Blocking.
    pub(crate) fn image_build(&self) -> Result<ImageBuild, Msg> {
        let status = self.container_status();
        let Some(cli) = self.cli_path(&status) else {
            return Err(status
                .hint
                .clone()
                .unwrap_or_else(|| msg!(ServerRunBuildNoRuntime)));
        };
        if !status.engine_running {
            return Err(status
                .hint
                .clone()
                .unwrap_or_else(|| msg!(ServerRunBuildRuntimeStopped)));
        }
        let source = self.agent_source();
        if !source.join("Containerfile").is_file() {
            return Err(msg!(
                ServerRunBuildContainerfileMissing,
                dir = source.display().to_string()
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
                lines: vec![msg!(ServerRunBuildStarting)],
            };
        }
        let s = self.clone();
        tokio::spawn(async move { build_task(s, cli, source, fingerprint).await });
        Ok(lock(&self.inner.agent.build).clone())
    }
}

// ---------- Build image ----------

/// Advice for the build error, from the last lines.
pub fn build_hint(lines: &[String]) -> Option<Msg> {
    let everything = lines.join("\n").to_lowercase();
    let contains = |s: &str| everything.contains(s);
    if contains("no space left") {
        Some(msg!(ServerRunBuildNoSpace))
    } else if contains("lookup ")
        || contains("no such host")
        || contains("temporary failure in name resolution")
        || contains("connection refused")
        || contains("timeout")
        || contains("tls handshake")
    {
        Some(msg!(ServerRunBuildNoNetwork))
    } else if contains("permission denied") && contains("docker.sock") {
        Some(msg!(ServerRunBuildDockerDenied))
    } else if contains("unauthorized") || contains("toomanyrequests") {
        Some(msg!(ServerRunBuildBaseImage))
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
    let done = |error: Option<Msg>| {
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
            done(Some(msg!(ServerRunBuildNotStarted, error = e.to_string())));
            return;
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
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
            // The output of the tool is read in one language, whatever the reader's: the hint
            // only looks for its keywords.
            let lines: Vec<String> = lock(&state.inner.agent.build)
                .lines
                .iter()
                .map(|m| m.render(cockpit_i18n::Lang::En))
                .collect();
            let last = lines.iter().rev().find(|z| !z.trim().is_empty()).cloned();
            let mut parts = vec![msg!(
                ServerRunBuildFailed,
                code = s.code().map_or("-".into(), |c| c.to_string())
            )];
            parts.extend(build_hint(&lines));
            if let Some(l) = last {
                parts.push(msg!(ServerRunLastMessage, line = l));
            }
            done(Some(msg!(
                ServerAgentJoined,
                parts = ArgValue::joined(parts, " ")
            )));
        }
        Err(e) => done(Some(msg!(ServerRunBuildInterrupted, error = e.to_string()))),
    }
}

pub(super) enum Strom {
    Off(BufReader<tokio::process::ChildStdout>),
    Error(BufReader<tokio::process::ChildStderr>),
}

impl Strom {
    async fn lines(self, tx: mpsc::UnboundedSender<Msg>) {
        async fn read<R: AsyncBufRead + Unpin>(mut r: R, tx: mpsc::UnboundedSender<Msg>) {
            let mut buffer = Vec::new();
            while let Ok(Some(g)) = read_line(&mut r, &mut buffer).await {
                let t = match g {
                    Read::Line(z) => {
                        let z = z.trim_end();
                        if z.is_empty() {
                            continue;
                        }
                        Msg::raw(z)
                    }
                    Read::TooLarge => msg!(ServerRunBuildLineSkipped),
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
