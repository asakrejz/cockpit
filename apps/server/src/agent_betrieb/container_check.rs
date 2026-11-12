use super::*;

impl AppState {
    // ---------- Erreichbarkeit aus dem Container ----------

    /// Prüft, ob ein Container den Endpunkt erreicht (kurzer Lauf des Images im Prüfmodus,
    /// `--rm`), und vergleicht mit dem, was der Mac sieht.
    pub(crate) async fn container_pruefen(&self) -> ContainerCheck {
        let s = self.clone();
        let vorbereitet = tokio::task::spawn_blocking(move || {
            let status = s.container_status();
            let cli = s.cli_pfad(&status).filter(|_| status.engine_running);
            let image = cli.as_ref().is_some_and(|c| s.image_vorhanden(c));
            (status, cli, image, s.agent_wahl(), s.agent_key())
        })
        .await;
        let Ok((status, cli, image, wahl, key)) = vorbereitet else {
            return ContainerCheck::default();
        };
        let nicht = |hinweis: &str| ContainerCheck {
            hint: Some(hinweis.into()),
            ..Default::default()
        };
        if let Some(m) = wahl.anbieter.not_implemented() {
            return nicht(&m);
        }
        let Some(endpunkt) = wahl.endpunkt else {
            return nicht(
                "Der Endpunkt ist nicht eingerichtet. Wähle zuerst Anbieter, Endpunkt und Modell.",
            );
        };
        let Some(key) = key else {
            return nicht(
                "Es fehlt der Key. Lege ihn zuerst ab, dann lässt sich der Endpunkt prüfen.",
            );
        };
        let Some(cli) = cli else {
            return nicht(&status.hint.unwrap_or_else(|| {
                "Die Container-Laufzeit läuft nicht (Schritt „Container“).".into()
            }));
        };
        if !image {
            return nicht(
                "Das Agent-Image fehlt. Baue es im Schritt „Container“ mit „Image bauen“, dann kann der Container testen.",
            );
        }
        let mac = cockpit_integrations::endpoint::verify(
            &self.endpunkt_ziel(),
            Some(&key),
            endpunkt.modell.as_deref(),
        )
        .await;
        let im_container = pruef_container(
            self,
            &cli,
            endpunkt.url.as_deref(),
            &key,
            endpunkt.modell.as_deref().unwrap_or("-"),
        )
        .await;
        let (erreicht, key_ok, text) = match &im_container {
            Ok(Ereignis::Pruefung {
                erreichbar,
                key_ok,
                hinweis,
            }) => (*erreichbar, *key_ok, hinweis.clone()),
            Ok(_) => (false, false, "Unerwartete Antwort des Containers.".into()),
            Err(m) => (false, false, m.clone()),
        };
        ContainerCheck {
            checked: true,
            mac_reached: mac.reachable,
            container_reached: erreicht,
            key_ok,
            hint: Some(pruefung_hinweis(mac.reachable, erreicht, key_ok, &text)),
        }
    }
}

/// Der Hinweis zur Prüfung aus dem Container, mit dem konkreten Rat, wenn der Mac den Proxy
/// erreicht und der Container nicht (typisch: VPN, DNS der Container-Maschine).
pub fn pruefung_hinweis(mac: bool, container: bool, key_ok: bool, text: &str) -> String {
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

/// Entfernt Container mit dem Label der Läufe (auch beendete). Gibt die Zahl zurück. Blockierend.
pub(super) fn container_labels_entfernen(cli: &Path) -> usize {
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

/// Beendet und entfernt einen Container.
pub(super) async fn container_entfernen(cli: &Path, name: &str) {
    cli_still(cli, &["rm", "-f", name]).await;
}

pub(super) fn pruef_auftrag_schreiben(ordner: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(ordner)?;
    std::fs::write(
        ordner.join("auftrag.json"),
        lauf::pruef_auftrag().to_string(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(ordner, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(
            ordner.join("auftrag.json"),
            std::fs::Permissions::from_mode(0o644),
        )?;
    }
    Ok(())
}

/// Setzt die Werte, die `-e NAME` weitergibt, und entfernt die nicht benutzten aus der geerbten Umgebung.
pub(super) fn kind_umgebung(
    cmd: &mut Command,
    endpunkt_url: Option<&str>,
    key: &str,
    modell: &str,
) {
    for (name, wert) in lauf::umgebung_fuer_kind(endpunkt_url, key, modell) {
        match wert {
            Some(w) => cmd.env(name, w),
            None => cmd.env_remove(name),
        };
    }
}

pub(super) async fn pruef_container(
    state: &AppState,
    cli: &Path,
    endpunkt_url: Option<&str>,
    key: &Secret,
    modell: &str,
) -> Result<Ereignis, String> {
    let ordner = state
        .agent_ordner()
        .join("pruefung")
        .join(jetzt_ms().to_string());
    pruef_auftrag_schreiben(&ordner).map_err(|e| format!("Auftrag nicht schreibbar: {e}"))?;
    let name = lauf::container_name("pruefung", jetzt_ms());
    let args = lauf::run_args(&LaufBefehl {
        name: &name,
        lauf: "pruefung",
        klon: &ordner,
        klon_nur_lesen: true,
        auftrag: &ordner,
        stdin_offen: false,
    });
    let mut cmd = Command::new(cli);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    kind_umgebung(&mut cmd, endpunkt_url, key.expose(), modell);
    let lauf_ = tokio::time::timeout(PRUEFUNG_ZEIT, cmd.output()).await;
    let ergebnis = match lauf_ {
        Err(_) => {
            container_entfernen(cli, &name).await;
            Err("Der Container hat nicht rechtzeitig geantwortet (60 Sekunden).".to_string())
        }
        Ok(Err(e)) => Err(format!("Der Container lässt sich nicht starten: {e}")),
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let ereignis = text
                .lines()
                .filter_map(|z| lauf::zeile_lesen(z).ok())
                .find(|e| matches!(e, Ereignis::Pruefung { .. } | Ereignis::Fehler { .. }));
            match ereignis {
                Some(Ereignis::Fehler { text }) => Err(text),
                Some(e) => Ok(e),
                None => Err(format!(
                    "Der Container hat nichts gemeldet (Code {}).{}",
                    out.status.code().map_or("-".into(), |c| c.to_string()),
                    letzte_zeile(&String::from_utf8_lossy(&out.stderr), &[key.expose()])
                        .map(|z| format!(" Letzte Meldung: {z}"))
                        .unwrap_or_default()
                )),
            }
        }
    };
    let _ = std::fs::remove_dir_all(&ordner);
    ergebnis
}

/// Die letzte nicht leere Zeile, ohne Geheimnisse und gekürzt.
pub(super) fn letzte_zeile(text: &str, geheim: &[&str]) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|z| !z.is_empty())
        .map(|z| {
            let z = agent_git::schwaerzen(z, geheim);
            if z.chars().count() > 300 {
                let mut k: String = z.chars().take(299).collect();
                k.push('…');
                k
            } else {
                z
            }
        })
}
