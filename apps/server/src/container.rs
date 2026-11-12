//! Container-Laufzeit erkennen (Docker oder Podman), nur über die Kommandozeile.
//!
//! Regeln (siehe „Container“ in `docs/ROADMAP.md`): Das Backend ruft `docker` oder `podman` als
//! Prozess auf, nie den Socket. Gewählt wird in dieser Reihenfolge: `COCKPIT_CONTAINER`,
//! `[agent] container` in `config.toml`, sonst erst `podman`, dann `docker` im `PATH`. Ob die
//! Engine läuft, zeigt `<cli> info`. Hier wird nichts gebaut oder gestartet.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cockpit_core::api::ContainerStatus;

/// So lange darf `<cli> info` dauern, bevor es als „läuft nicht“ gilt.
const INFO_ZEIT: Duration = Duration::from_secs(8);
/// So lange gilt ein Ergebnis, bevor `/api/health` neu fragt (`info` kann Sekunden dauern).
const GUELTIG: Duration = Duration::from_secs(10);
/// Reihenfolge der Suche ohne ausdrückliche Wahl.
const SUCHE: [&str; 2] = ["podman", "docker"];

/// Die Wahl aus Umgebung und Datei; die Umgebung geht vor.
fn gewaehlt(konfig: Option<&str>, env: Option<&str>) -> Option<(String, &'static str)> {
    env.map(|l| (l.to_string(), "umgebung"))
        .or_else(|| konfig.map(|l| (l.to_string(), "konfiguration")))
}

fn nicht_gefunden(cli: &str) -> String {
    format!(
        "„{cli}“ steht nicht im PATH. Installiere es (Docker: Rancher Desktop mit Engine „dockerd (moby)“; Podman: brew install podman) oder wähle die andere Laufzeit."
    )
}

fn nicht_gestartet(cli: &str) -> String {
    match cli {
        "podman" => "„podman info“ schlägt fehl. Starte die Maschine mit „podman machine start“.".into(),
        _ => "„docker info“ schlägt fehl. Starte Rancher Desktop und stelle die Engine auf „dockerd (moby)“ (nicht „containerd“).".into(),
    }
}

/// Die Entscheidung als reine Funktion: `vorhanden(cli)` sagt, ob das Programm im `PATH` liegt,
/// `laeuft(cli)`, ob `<cli> info` gelingt.
pub fn waehle(
    konfig: Option<&str>,
    env: Option<&str>,
    vorhanden: impl Fn(&str) -> bool,
    laeuft: impl Fn(&str) -> bool,
) -> ContainerStatus {
    if let Some((cli, quelle)) = gewaehlt(konfig, env) {
        if !vorhanden(&cli) {
            return ContainerStatus {
                hint: Some(nicht_gefunden(&cli)),
                runtime: Some(cli),
                engine_running: false,
                source: quelle.into(),
            };
        }
        let engine_laeuft = laeuft(&cli);
        return ContainerStatus {
            hint: (!engine_laeuft).then(|| nicht_gestartet(&cli)),
            runtime: Some(cli),
            engine_running: engine_laeuft,
            source: quelle.into(),
        };
    }
    let mut erste = None;
    for cli in SUCHE {
        if !vorhanden(cli) {
            continue;
        }
        if laeuft(cli) {
            return ContainerStatus {
                runtime: Some(cli.into()),
                engine_running: true,
                source: "erkannt".into(),
                hint: None,
            };
        }
        erste.get_or_insert(cli);
    }
    match erste {
        Some(cli) => ContainerStatus {
            runtime: Some(cli.into()),
            engine_running: false,
            source: "erkannt".into(),
            hint: Some(nicht_gestartet(cli)),
        },
        None => ContainerStatus {
            runtime: None,
            engine_running: false,
            source: "keine".into(),
            hint: Some(
                "Weder podman noch docker im PATH. Installiere Rancher Desktop (Engine „dockerd (moby)“) oder Podman."
                    .into(),
            ),
        },
    }
}

/// Sucht ein ausführbares Programm in den Ordnern von `path`.
pub fn im_pfad(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|ordner| ordner.join(name))
        .find(|datei| ausfuehrbar(datei))
}

#[cfg(unix)]
fn ausfuehrbar(datei: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    datei
        .metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn ausfuehrbar(datei: &std::path::Path) -> bool {
    datei.is_file()
}

/// `<programm> info` ohne Ausgabe; gelingt, wenn es mit Status 0 endet. Nach `INFO_ZEIT` wird
/// der Prozess beendet. Blockierend.
fn info_gelingt(programm: &std::path::Path) -> bool {
    let Ok(mut kind) = Command::new(programm)
        .arg("info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();
    loop {
        match kind.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < INFO_ZEIT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = kind.kill();
                let _ = kind.wait();
                return false;
            }
        }
    }
}

/// Erkennt die Laufzeit mit echten Prozessaufrufen. Blockierend.
pub fn erkennen(konfig: Option<&str>, env: Option<&str>, path: &OsStr) -> ContainerStatus {
    waehle(
        konfig,
        env,
        |cli| im_pfad(cli, path).is_some(),
        |cli| im_pfad(cli, path).is_some_and(|p| info_gelingt(&p)),
    )
}

/// Merkt sich das Ergebnis kurz, damit `/api/health` nicht bei jedem Aufruf `info` startet.
#[derive(Default)]
pub struct Zwischenspeicher {
    stand: Mutex<Option<(Instant, ContainerStatus)>>,
}

impl Zwischenspeicher {
    /// Das gemerkte Ergebnis oder ein frisches. Blockierend.
    pub fn holen(&self, frisch: impl FnOnce() -> ContainerStatus) -> ContainerStatus {
        let mut stand = self.stand.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((zeit, status)) = stand.as_ref()
            && zeit.elapsed() < GUELTIG
        {
            return status.clone();
        }
        let neu = frisch();
        *stand = Some((Instant::now(), neu.clone()));
        neu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alle(_: &str) -> bool {
        true
    }
    fn keine(_: &str) -> bool {
        false
    }

    #[test]
    fn ohne_wahl_kommt_podman_vor_docker() {
        let s = waehle(None, None, alle, alle);
        assert_eq!(s.runtime.as_deref(), Some("podman"));
        assert!(s.engine_running && s.hint.is_none());
        assert_eq!(s.source, "erkannt");
    }

    #[test]
    fn laeuft_podman_nicht_wird_docker_genommen() {
        let s = waehle(None, None, alle, |cli| cli == "docker");
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert!(s.engine_running);
        let s = waehle(None, None, |cli| cli == "docker", alle);
        assert_eq!(s.runtime.as_deref(), Some("docker"));
    }

    #[test]
    fn vorhanden_aber_ohne_engine_gibt_einen_hinweis() {
        let s = waehle(None, None, alle, keine);
        assert_eq!(s.runtime.as_deref(), Some("podman"), "die erste gefundene");
        assert!(!s.engine_running);
        assert!(s.hint.unwrap().contains("podman machine start"));
        let s = waehle(None, None, |c| c == "docker", keine);
        assert!(s.hint.unwrap().contains("dockerd (moby)"));
    }

    #[test]
    fn ohne_beides_gibt_es_keine_laufzeit() {
        let s = waehle(None, None, keine, keine);
        assert_eq!(s.runtime, None);
        assert_eq!(s.source, "keine");
        assert!(s.hint.unwrap().contains("Weder podman noch docker"));
    }

    #[test]
    fn ausdrueckliche_wahl_geht_vor_und_umgebung_vor_datei() {
        let s = waehle(Some("podman"), Some("docker"), alle, alle);
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert_eq!(s.source, "umgebung");
        let s = waehle(Some("docker"), None, alle, alle);
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert_eq!(s.source, "konfiguration");
        // Gewählt heißt gewählt: Es wird nicht auf die andere Laufzeit ausgewichen.
        let s = waehle(Some("docker"), None, alle, |cli| cli == "podman");
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert!(!s.engine_running);
        let s = waehle(Some("docker"), None, |cli| cli == "podman", alle);
        assert!(s.hint.unwrap().contains("nicht im PATH"));
    }

    #[cfg(unix)]
    mod prozess {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn programm(ordner: &std::path::Path, name: &str, skript: &str) {
            let datei = ordner.join(name);
            std::fs::write(&datei, format!("#!/bin/sh\n{skript}\n")).unwrap();
            std::fs::set_permissions(&datei, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        #[test]
        fn ruft_info_auf_und_wertet_den_status() {
            let ordner = tempfile::tempdir().unwrap();
            programm(
                ordner.path(),
                "podman",
                "[ \"$1\" = info ] && exit 1; exit 2",
            );
            programm(
                ordner.path(),
                "docker",
                "[ \"$1\" = info ] && exit 0; exit 2",
            );
            let s = erkennen(None, None, ordner.path().as_os_str());
            assert_eq!(s.runtime.as_deref(), Some("docker"));
            assert!(s.engine_running);
            let s = erkennen(Some("podman"), None, ordner.path().as_os_str());
            assert_eq!(s.runtime.as_deref(), Some("podman"));
            assert!(!s.engine_running);
        }

        #[test]
        fn eine_datei_ohne_ausfuehrrecht_zaehlt_nicht() {
            let ordner = tempfile::tempdir().unwrap();
            std::fs::write(ordner.path().join("docker"), "#!/bin/sh\nexit 0\n").unwrap();
            let s = erkennen(None, None, ordner.path().as_os_str());
            assert_eq!(s.runtime, None);
        }
    }

    #[test]
    fn zwischenspeicher_fragt_nur_einmal() {
        let speicher = Zwischenspeicher::default();
        let mut zaehler = 0;
        for _ in 0..3 {
            speicher.holen(|| {
                zaehler += 1;
                waehle(None, None, keine, keine)
            });
        }
        assert_eq!(zaehler, 1);
    }
}
