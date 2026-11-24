use cockpit_i18n::{ArgValue, Msg, msg};

use super::*;
use image_build::status_hint;

impl AppState {
    // ---------- Reachability from the container ----------

    /// Checks whether a container reaches the endpoint (short run of the image in check mode,
    /// `--rm`), and compares with what the Mac sees.
    pub(crate) async fn container_check(&self) -> ContainerCheck {
        let s = self.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let status = s.container_status();
            let cli = s.cli_path(&status).filter(|_| status.engine_running);
            let image = cli.as_ref().map(|c| s.image_probe(c));
            (status, cli, image, s.agent_choice(), s.agent_key())
        })
        .await;
        let Ok((status, cli, image, choice, key)) = prepared else {
            return ContainerCheck::default();
        };
        let not = |hint: Msg| ContainerCheck {
            hint: Some(hint.de()),
            hint_msg: Some(hint),
            ..Default::default()
        };
        let Some(endpoint) = choice.endpoint else {
            return not(msg!(ServerRunCheckEndpointMissing));
        };
        let Some(key) = key else {
            return not(msg!(ServerRunCheckKeyMissing));
        };
        let Some(cli) = cli else {
            return not(status_hint(&status).unwrap_or_else(|| msg!(ServerRunCheckRuntimeDown)));
        };
        if let Some(problem) = image.and_then(|p| agent_run::image_problem(p.state, p.hint)) {
            return not(msg!(ServerRunCheckThenTest, problem = problem));
        }
        let mac = cockpit_integrations::endpoint::verify(
            &self.endpoint_target(),
            Some(&key),
            endpoint.model.as_deref(),
        )
        .await;
        let in_container = check_container(
            self,
            &cli,
            choice.provider,
            endpoint.url.as_deref(),
            &key,
            endpoint.model.as_deref().unwrap_or("-"),
            self.agent_io(&status),
        )
        .await;
        let (reached, key_ok, text) = match &in_container {
            Ok(Event::Check {
                reachable,
                key_ok,
                hint,
            }) => (*reachable, *key_ok, Msg::raw(hint)),
            Ok(_) => (false, false, msg!(ServerRunCheckUnexpected)),
            Err(m) => (false, false, m.clone()),
        };
        let hint = check_hint(mac.reachable, reached, key_ok, text);
        ContainerCheck {
            checked: true,
            mac_reached: mac.reachable,
            container_reached: reached,
            key_ok,
            hint: Some(hint.de()),
            hint_msg: Some(hint),
        }
    }
}

/// The hint about the check from the container, with the concrete advice if the Mac reaches the proxy
/// and the container does not (typical: VPN, DNS of the container machine).
pub fn check_hint(mac: bool, container: bool, key_ok: bool, text: Msg) -> Msg {
    match (mac, container, key_ok) {
        (true, true, true) => msg!(ServerRunCheckOk),
        (_, true, false) => msg!(ServerRunCheckKeyRejected, text = text),
        (true, false, _) => msg!(ServerRunCheckContainerCannot, text = text),
        (false, false, _) => msg!(ServerRunCheckNeither, text = text),
        (false, true, true) => msg!(ServerRunCheckMacCannot),
    }
}

// ---------- Container helpers ----------

/// Removes containers with the label of the runs (also ended ones). Returns the number. Blocking.
pub(super) fn container_labels_remove(cli: &Path) -> usize {
    let Ok(out) = std::process::Command::new(cli)
        .args(["ps", "-a", "-q", "--filter", &format!("label={LABEL}")])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return 0;
    };
    let ids: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if ids.is_empty() {
        return 0;
    }
    let _ = std::process::Command::new(cli)
        .args(["rm", "-f"])
        .args(&ids)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    ids.len()
}

pub(super) async fn cli_still(cli: &Path, args: &[&str]) {
    let _ = Command::new(cli)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// Ends and removes a container.
pub(super) async fn container_remove(cli: &Path, name: &str) {
    cli_still(cli, &["rm", "-f", name]).await;
}

pub(super) fn check_job_write(dir: &Path, language: cockpit_i18n::Lang) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("job.json"),
        agent_run::check_job(language).to_string(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(dir.join("job.json"), std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

/// Sets the values that `-e NAME` passes on and removes the unused ones from the inherited environment.
pub(super) fn kind_env(
    cmd: &mut Command,
    provider: Provider,
    endpoint_url: Option<&str>,
    key: &str,
    model: &str,
) {
    for (name, value) in agent_run::env_for_kind(provider, endpoint_url, key, model) {
        match value {
            Some(w) => cmd.env(name, w),
            None => cmd.env_remove(name),
        };
    }
}

pub(super) async fn check_container(
    state: &AppState,
    cli: &Path,
    provider: Provider,
    endpoint_url: Option<&str>,
    key: &Secret,
    model: &str,
    io: agent_run::AgentIo,
) -> Result<Event, Msg> {
    let dir = state.agent_dir().join("check").join(now_ms().to_string());
    // The runner writes its hint in the language of the interface.
    let language = state.inner.live.copy().language.unwrap_or_default();
    check_job_write(&dir, language)
        .map_err(|e| msg!(ServerRunCheckJobNotWritable, error = e.to_string()))?;
    let name = agent_run::container_name("check", now_ms());
    let args = agent_run::run_args(&RunCommand {
        name: &name,
        run: "check",
        clone: &dir,
        clone_only_read: true,
        job: &dir,
        stdin_open: false,
        provider,
        io,
    });
    let mut cmd = Command::new(cli);
    cmd.args(&args);
    kind_env(&mut cmd, provider, endpoint_url, key.expose(), model);
    if io == agent_run::AgentIo::Copy {
        if let Err(m) = copy_io::prepare(cli, cmd, &name, &dir, &dir).await {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(m);
        }
        cmd = Command::new(cli);
        cmd.args(agent_run::start_args(&name, false));
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let run = tokio::time::timeout(CHECK_TIME, cmd.output()).await;
    let result = match run {
        Err(_) => {
            container_remove(cli, &name).await;
            Err(msg!(ServerRunCheckTimeout))
        }
        Ok(Err(e)) => Err(msg!(ServerRunContainerNotStarted, error = e.to_string())),
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let event = text
                .lines()
                .filter_map(|z| agent_run::line_read(z).ok())
                .find(|e| matches!(e, Event::Check { .. } | Event::Error { .. }));
            match event {
                Some(Event::Error { text }) => Err(Msg::raw(text)),
                Some(e) => Ok(e),
                None => {
                    let mut parts = vec![msg!(
                        ServerRunCheckNoReport,
                        code = out.status.code().map_or("-".into(), |c| c.to_string())
                    )];
                    if let Some(z) =
                        last_line(&String::from_utf8_lossy(&out.stderr), &[key.expose()])
                    {
                        parts.push(msg!(ServerRunLastMessage, line = z));
                    }
                    Err(msg!(
                        ServerAgentJoined,
                        parts = ArgValue::joined(parts, " ")
                    ))
                }
            }
        }
    };
    if io == agent_run::AgentIo::Copy {
        container_remove(cli, &name).await;
    }
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The last non-empty line, without secrets and truncated.
pub(super) fn last_line(text: &str, secret: &[&str]) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|z| !z.is_empty())
        .map(|z| {
            let z = agent_git::redact(z, secret);
            if z.chars().count() > 300 {
                let mut k: String = z.chars().take(299).collect();
                k.push('…');
                k
            } else {
                z
            }
        })
}
