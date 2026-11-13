use super::*;

impl AppState {
    // ---------- Reachability from the container ----------

    /// Checks whether a container reaches the endpoint (short run of the image in check mode,
    /// `--rm`), and compares with what the Mac sees.
    pub(crate) async fn container_check(&self) -> ContainerCheck {
        let s = self.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let status = s.container_status();
            let cli = s.cli_path(&status).filter(|_| status.engine_running);
            let image = cli.as_ref().is_some_and(|c| s.image_present(c));
            (status, cli, image, s.agent_choice(), s.agent_key())
        })
        .await;
        let Ok((status, cli, image, choice, key)) = prepared else {
            return ContainerCheck::default();
        };
        let not = |hint: &str| ContainerCheck {
            hint: Some(hint.into()),
            ..Default::default()
        };
        if let Some(m) = choice.provider.not_implemented() {
            return not(&m);
        }
        let Some(endpoint) = choice.endpoint else {
            return not(
                "Der Endpunkt ist nicht eingerichtet. Wähle zuerst Anbieter, Endpunkt und Modell.",
            );
        };
        let Some(key) = key else {
            return not(
                "Es fehlt der Key. Lege ihn zuerst ab, dann lässt sich der Endpunkt prüfen.",
            );
        };
        let Some(cli) = cli else {
            return not(&status.hint.unwrap_or_else(|| {
                "Die Container-Laufzeit läuft nicht (Schritt „Container“).".into()
            }));
        };
        if !image {
            return not(
                "Das Agent-Image fehlt. Baue es im Schritt „Container“ mit „Image bauen“, dann kann der Container testen.",
            );
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
            endpoint.url.as_deref(),
            &key,
            endpoint.model.as_deref().unwrap_or("-"),
        )
        .await;
        let (reached, key_ok, text) = match &in_container {
            Ok(Event::Check {
                reachable,
                key_ok,
                hint,
            }) => (*reachable, *key_ok, hint.clone()),
            Ok(_) => (false, false, "Unerwartete Antwort des Containers.".into()),
            Err(m) => (false, false, m.clone()),
        };
        ContainerCheck {
            checked: true,
            mac_reached: mac.reachable,
            container_reached: reached,
            key_ok,
            hint: Some(check_hint(mac.reachable, reached, key_ok, &text)),
        }
    }
}

/// The hint about the check from the container, with the concrete advice if the Mac reaches the proxy
/// and the container does not (typical: VPN, DNS of the container machine).
pub fn check_hint(mac: bool, container: bool, key_ok: bool, text: &str) -> String {
    match (mac, container, key_ok) {
        (true, true, true) => "Der Proxy ist auch aus dem Container erreichbar und nimmt den Key an.".into(),
        (_, true, false) => format!(
            "Der Container erreicht den Proxy, aber der Key wird abgelehnt. {text}"
        ),
        (true, false, _) => format!(
            "Dein Mac erreicht den Proxy, der Container aber nicht. {text} Das liegt meist daran, dass die Container-Maschine (Rancher Desktop bzw. Podman) die VPN-Verbindung oder den Namen des Proxys nicht kennt. Verbinde zuerst das VPN und starte dann Rancher Desktop bzw. „podman machine stop“ und „podman machine start“ neu. Hilft das nicht, frag deine IT, ob der Proxy aus Container-Maschinen erreichbar ist, oder trage in config.toml die IP-Adresse statt des Namens ein."
        ),
        (false, false, _) => format!(
            "Weder dein Mac noch der Container erreichen den Proxy. Prüfe Adresse und VPN. {text}"
        ),
        (false, true, true) => "Der Container erreicht den Proxy, dein Mac nicht – ungewöhnlich. Prüfe die Adresse in config.toml.".into(),
    }
}

// ---------- Container-Hilfen ----------

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

pub(super) fn check_job_write(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("job.json"), agent_run::check_job().to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(dir.join("job.json"), std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

/// Sets the values that `-e NAME` passes on and removes the unused ones from the inherited environment.
pub(super) fn kind_env(cmd: &mut Command, endpoint_url: Option<&str>, key: &str, model: &str) {
    for (name, wert) in agent_run::env_for_kind(endpoint_url, key, model) {
        match wert {
            Some(w) => cmd.env(name, w),
            None => cmd.env_remove(name),
        };
    }
}

pub(super) async fn check_container(
    state: &AppState,
    cli: &Path,
    endpoint_url: Option<&str>,
    key: &Secret,
    model: &str,
) -> Result<Event, String> {
    let dir = state.agent_dir().join("check").join(now_ms().to_string());
    check_job_write(&dir).map_err(|e| format!("Auftrag nicht schreibbar: {e}"))?;
    let name = agent_run::container_name("check", now_ms());
    let args = agent_run::run_args(&RunCommand {
        name: &name,
        run: "check",
        clone: &dir,
        clone_only_read: true,
        job: &dir,
        stdin_open: false,
    });
    let mut cmd = Command::new(cli);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    kind_env(&mut cmd, endpoint_url, key.expose(), model);
    let run = tokio::time::timeout(CHECK_TIME, cmd.output()).await;
    let result = match run {
        Err(_) => {
            container_remove(cli, &name).await;
            Err("Der Container hat nicht rechtzeitig geantwortet (60 Sekunden).".to_string())
        }
        Ok(Err(e)) => Err(format!("Der Container lässt sich nicht starten: {e}")),
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let event = text
                .lines()
                .filter_map(|z| agent_run::line_read(z).ok())
                .find(|e| matches!(e, Event::Check { .. } | Event::Error { .. }));
            match event {
                Some(Event::Error { text }) => Err(text),
                Some(e) => Ok(e),
                None => Err(format!(
                    "Der Container hat nichts gemeldet (Code {}).{}",
                    out.status.code().map_or("-".into(), |c| c.to_string()),
                    last_line(&String::from_utf8_lossy(&out.stderr), &[key.expose()])
                        .map(|z| format!(" Letzte Meldung: {z}"))
                        .unwrap_or_default()
                )),
            }
        }
    };
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
