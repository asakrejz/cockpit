//! Der echte Agent, ausführender Teil: Image bauen, Container starten und überwachen, Ereignisse
//! anwenden, aufräumen. Entscheidungen fallen in `cockpit-core` (Aktionen, Grenzen, Regeln) und in
//! `agent_lauf` (rein); hier laufen nur die Prozesse.
//!
//! Ein Lauf ist ein Container, gestartet mit `docker run` bzw. `podman run` (siehe `agent_lauf`).
//! Das Backend liest dessen stdout zeilenweise, verwirft Unbrauchbares und wendet den Rest als
//! `Action::ReportAgent` (Urheber Agent) an. Stoppen heißt `rm -f` auf den Container.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cockpit_core::api::{AgentStatus, ContainerCheck, ContainerStatus, ImageBuild};
use cockpit_core::{Action, ActionError, Actor, AgentReport, RunKind, RunState, Workspace};
use cockpit_integrations::Secret;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent_git;
use crate::agent_lauf::{
    self as lauf, Anweisung, Ereignis, IMAGE, Info, LABEL, LaufBefehl, MAX_ZEILE, Modus,
    Voraussetzungen,
};
use crate::app::{AppState, jetzt_ms};
use crate::config::Seed;
use crate::container;
use crate::live::Fehler;

/// So oft vergleicht der Aufpasser Läufe und Container.
pub const AUFPASSER_TAKT: Duration = Duration::from_secs(1);
/// So lange gilt „Image vorhanden?“ ohne neue Frage.
const IMAGE_GUELTIG: Duration = Duration::from_secs(5);
/// Nach einem Fehler startet der Aufpasser denselben Lauf so lange nicht neu.
const SPERRE_NACH_FEHLER: Duration = Duration::from_secs(30);
/// Zeit über das Limit des Laufs hinaus, bevor das Backend den Container selbst beendet.
const NACHSPIELZEIT_S: u64 = 120;
/// Ohne Limit gilt diese Obergrenze für einen Lauf.
const STANDARD_MAXIMAL_S: u64 = 2 * 60 * 60;
/// So lange darf ein arbeitender Container nichts von sich hören lassen.
const STILLE_GRENZE_S: u64 = 15 * 60;
/// Dauer der Erreichbarkeitsprüfung im Container.
const PRUEFUNG_ZEIT: Duration = Duration::from_secs(60);
const MAX_BAU_ZEILEN: usize = 200;

/// So lange darf der Container höchstens laufen: das Zeitlimit des Laufs (samt Erhöhung) plus
/// etwas Spielraum, damit zuerst die Grenze in `cockpit-core` greift. Ohne Limit gilt eine Obergrenze.
pub fn erlaubte_sekunden(run_minutes: Option<u32>, extra_minutes: u64) -> u64 {
    run_minutes.map_or(STANDARD_MAXIMAL_S, |m| {
        (u64::from(m) + extra_minutes).saturating_mul(60)
    }) + NACHSPIELZEIT_S
}

/// Befehle an die Aufgabe eines Laufs.
#[derive(Debug)]
pub enum Befehl {
    /// Container beenden, den Lauf nicht anfassen.
    Stoppen,
    Pausieren,
    Fortsetzen,
    Antwort(String),
    Hinweis(String),
}

struct Handle {
    info: Arc<Mutex<Info>>,
    tx: mpsc::UnboundedSender<Befehl>,
}

/// Zustand des Agenten im Server.
#[derive(Default)]
pub struct Laeufe {
    laufend: Mutex<HashMap<String, Handle>>,
    gescheitert: Mutex<HashMap<String, Instant>>,
    bau: Mutex<ImageBuild>,
    image: Mutex<Option<(Instant, bool)>>,
}

fn sperre<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Alles, was ein Lauf an Zugang braucht (aus Einstellungen und Schlüsselbund).
struct Zugang {
    cli: PathBuf,
    /// `None` = direkt beim Anbieter.
    endpunkt_url: Option<String>,
    key: Secret,
    modell: String,
    repo: String,
    git_basis: String,
    github: Secret,
}

enum Ende {
    /// Beendet, ohne den Lauf zu ändern (abgebrochen, angehalten).
    Ruhig,
    Fehler(String),
    Fertig(String),
}

impl AppState {
    /// Läuft der echte Agent (Container) oder die Simulation (Seed `demo`, Tests)?
    pub(crate) fn agent_echt(&self) -> bool {
        self.inner.einstellungen.seed != Seed::Demo
    }

    fn suchpfad(&self) -> OsString {
        self.inner
            .einstellungen
            .suchpfad
            .clone()
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default()
    }

    /// Die Container-Laufzeit, kurz gemerkt. Blockierend.
    pub(crate) fn container_status(&self) -> ContainerStatus {
        let e = &self.inner.einstellungen;
        let pfad = self.suchpfad();
        self.inner.container.holen(|| {
            container::erkennen(e.container.as_deref(), e.container_env.as_deref(), &pfad)
        })
    }

    fn cli_pfad(&self, status: &ContainerStatus) -> Option<PathBuf> {
        container::im_pfad(status.runtime.as_deref()?, &self.suchpfad())
    }

    /// Gibt es das Agent-Image? (`image inspect`, kurz gemerkt). Blockierend.
    fn image_vorhanden(&self, cli: &Path) -> bool {
        let mut c = sperre(&self.inner.agent.image);
        if let Some((t, v)) = *c
            && t.elapsed() < IMAGE_GUELTIG
        {
            return v;
        }
        let ok = std::process::Command::new(cli)
            .args(["image", "inspect", IMAGE])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        *c = Some((Instant::now(), ok));
        ok
    }

    fn image_vergessen(&self) {
        *sperre(&self.inner.agent.image) = None;
    }

    /// Der Key des Endpunkts: `agent-key`, ersatzweise das ältere `litellm`.
    fn agent_key(&self) -> Option<Secret> {
        match crate::app::agent_key_lesen(self.inner.secrets.as_ref()) {
            Ok(k) => k.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Key des Agent-Endpunkts nicht lesbar: {e}");
                None
            }
        }
    }

    fn repo_name(&self) -> Result<String, String> {
        let q = self.quellen();
        let github: Vec<String> = q
            .github
            .as_ref()
            .map(|g| g.repos.clone())
            .unwrap_or_default();
        lauf::repo_waehlen(self.inner.einstellungen.agent_repo.as_deref(), &github)
            .map(str::to_string)
    }

    /// Der Stand des Agenten für `/api/health`. Blockierend.
    pub(crate) fn agent_status(&self) -> AgentStatus {
        let container = self.container_status();
        let image = self
            .cli_pfad(&container)
            .filter(|_| container.engine_running)
            .is_some_and(|cli| self.image_vorhanden(&cli));
        let wahl = self.agent_wahl();
        let repo = self.repo_name();
        let v = Voraussetzungen {
            container: &container,
            image,
            anbieter: wahl.anbieter,
            modell: wahl.endpunkt.as_ref().is_some_and(|e| e.modell.is_some()),
            key: self.agent_key().is_some(),
            repo: repo.as_deref().map_err(Clone::clone),
            github_zugang: self.github_zugang().is_some(),
        };
        lauf::status(self.agent_echt(), &v)
    }

    /// Alles für einen Lauf, oder die Liste dessen, was fehlt. Blockierend.
    fn zugang(&self) -> Result<Zugang, Vec<String>> {
        let st = self.agent_status();
        if !st.ready {
            return Err(st.missing);
        }
        let container = self.container_status();
        let wahl = self.agent_wahl();
        if let Some(m) = wahl.anbieter.not_implemented() {
            return Err(vec![m]);
        }
        let endpunkt = wahl
            .endpunkt
            .ok_or_else(|| vec!["Der Modellzugang fehlt.".to_string()])?;
        let q = self.quellen();
        let api = q
            .github
            .as_ref()
            .map_or(crate::config::STANDARD_GITHUB_API, |g| g.api_url.as_str())
            .to_string();
        Ok(Zugang {
            cli: self.cli_pfad(&container).ok_or_else(|| {
                vec!["Das Container-Programm ist nicht mehr zu finden.".to_string()]
            })?,
            endpunkt_url: endpunkt.url,
            key: self
                .agent_key()
                .ok_or_else(|| vec!["Der Key des Modellzugangs fehlt.".to_string()])?,
            modell: endpunkt
                .modell
                .ok_or_else(|| vec!["Der Modellname fehlt.".to_string()])?,
            repo: st.repo.ok_or_else(|| vec!["Das Repo fehlt.".to_string()])?,
            git_basis: self
                .inner
                .einstellungen
                .git_basis
                .clone()
                .unwrap_or_else(|| agent_git::basis_aus_api(&api)),
            github: self
                .github_zugang()
                .ok_or_else(|| vec!["GitHub ist nicht verbunden.".to_string()])?,
        })
    }

    fn agent_ordner(&self) -> PathBuf {
        self.inner
            .einstellungen
            .daten_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("cockpit"))
            .join("agent")
    }

    /// Der Ordner mit `Containerfile` und Läufer.
    fn agent_quelle(&self) -> PathBuf {
        self.inner
            .einstellungen
            .agent_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("apps/agent"))
    }

    /// Ablehnungen vor dem Anwenden einer Aktion im echten Modus: Läufe starten nur, wenn alles
    /// eingerichtet ist, und Folgeaufträge gibt es noch nicht. `None`, wenn nichts dagegen spricht.
    /// Blockierend.
    pub(crate) fn agent_sperre(&self, action: &Action) -> Option<String> {
        if !self.agent_echt() {
            return None;
        }
        match action {
            Action::StartAgent { .. } | Action::ResumeStopped { .. } => {
                let st = self.agent_status();
                (!st.ready).then(|| {
                    format!(
                        "Der Agent kann noch nicht starten. {}",
                        st.missing.join(" ")
                    )
                })
            }
            Action::ApprovePlan { ticket } => {
                let ws = self.inner.live.kopie();
                let wartet = ws.runs.iter().any(|r| {
                    r.key() == *ticket && r.is_awaiting() && r.agent_plan.is_none()
                });
                wartet.then(|| {
                    "Der Agent erarbeitet den Plan noch. Warte, bis er vorliegt – dann kannst du ihn freigeben."
                        .to_string()
                })
            }
            Action::AddressReview { .. } | Action::InvestigateCheck { .. } | Action::ApplyFix { .. } => {
                Some(
                    "Folgeaufträge (Kommentare umsetzen, Checks untersuchen) laufen noch nicht mit dem echten Agenten. Setze die Änderungen vorerst selbst um."
                        .to_string(),
                )
            }
            _ => None,
        }
    }

    // ---------- Meldungen anwenden ----------

    /// Wendet eine Meldung des Agenten an. Blockierend.
    fn melden_blockierend(&self, key: &str, report: AgentReport) -> Result<(), Fehler> {
        self.inner
            .live
            .anwenden(
                &self.inner.db,
                Action::ReportAgent {
                    run: key.to_string(),
                    report,
                },
                Actor::Agent,
                jetzt_ms(),
            )
            .map(|_| ())
    }

    async fn melden(&self, key: &str, report: AgentReport) -> Result<(), Fehler> {
        let s = self.clone();
        let k = key.to_string();
        tokio::task::spawn_blocking(move || s.melden_blockierend(&k, report))
            .await
            .unwrap_or_else(|_| Err(Fehler::Speichern("Interner Fehler".into())))
    }

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

    fn befehl(&self, key: &str, b: Befehl) {
        if let Some(h) = sperre(&self.inner.agent.laufend).get(key) {
            let _ = h.tx.send(b);
        }
    }

    fn lauf_starten(&self, key: &str, modus: Modus) {
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
fn container_labels_entfernen(cli: &Path) -> usize {
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

async fn cli_still(cli: &Path, args: &[&str]) {
    let _ = Command::new(cli)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// Beendet und entfernt einen Container.
async fn container_entfernen(cli: &Path, name: &str) {
    cli_still(cli, &["rm", "-f", name]).await;
}

fn pruef_auftrag_schreiben(ordner: &Path) -> std::io::Result<()> {
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
fn kind_umgebung(cmd: &mut Command, endpunkt_url: Option<&str>, key: &str, modell: &str) {
    for (name, wert) in lauf::umgebung_fuer_kind(endpunkt_url, key, modell) {
        match wert {
            Some(w) => cmd.env(name, w),
            None => cmd.env_remove(name),
        };
    }
}

async fn pruef_container(
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
fn letzte_zeile(text: &str, geheim: &[&str]) -> Option<String> {
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

async fn bau_aufgabe(state: AppState, cli: PathBuf, quelle: PathBuf) {
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

enum Strom {
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

// ---------- Zeilen lesen (begrenzt) ----------

pub enum Gelesen {
    Zeile(String),
    ZuGross,
}

/// Liest eine Zeile, ohne mehr als `MAX_ZEILE` Byte zu puffern: Eine längere Zeile wird bis zum
/// Zeilenende überlesen und als verworfen gemeldet. `None` am Ende des Stroms.
pub async fn lies_zeile<R: AsyncBufRead + Unpin>(
    r: &mut R,
    puffer: &mut Vec<u8>,
) -> std::io::Result<Option<Gelesen>> {
    puffer.clear();
    let mut zu_gross = false;
    let mut gelesen_etwas = false;
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            if !gelesen_etwas {
                return Ok(None);
            }
            break;
        }
        gelesen_etwas = true;
        let (nutz, ende) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (&chunk[..i], Some(i + 1)),
            None => (chunk, None),
        };
        if !zu_gross {
            if puffer.len() + nutz.len() > MAX_ZEILE {
                zu_gross = true;
                puffer.clear();
            } else {
                puffer.extend_from_slice(nutz);
            }
        }
        let verbrauch = ende.unwrap_or(chunk.len());
        r.consume(verbrauch);
        if ende.is_some() {
            break;
        }
    }
    Ok(Some(if zu_gross {
        Gelesen::ZuGross
    } else {
        Gelesen::Zeile(String::from_utf8_lossy(puffer).into_owned())
    }))
}

// ---------- Ein Lauf ----------

fn auftrag_schreiben(ordner: &Path, inhalt: &serde_json::Value) -> std::io::Result<()> {
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

async fn lauf_aufgabe(
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

enum Anwendung {
    Ok,
    /// Der Lauf ist nicht mehr aktiv oder die Meldung passt nicht (mehr).
    Abgewiesen,
    Fehler,
}

async fn anwenden(state: &AppState, key: &str, r: AgentReport) -> Anwendung {
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

fn befehlszeile(t: &str, text: &str) -> String {
    format!("{}\n", serde_json::json!({ "t": t, "text": text }))
}

#[allow(clippy::too_many_lines)]
async fn lauf_ausfuehren(
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

fn lauf_aktiv(state: &AppState, key: &str) -> bool {
    state
        .inner
        .live
        .kopie()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_active())
}

/// Der Lauf ist nicht mehr aktiv oder wurde von einer Grenze angehalten: Der Container soll enden.
fn lauf_angehalten_oder_weg(state: &AppState, key: &str) -> bool {
    !state
        .inner
        .live
        .kopie()
        .runs
        .iter()
        .any(|r| r.key() == key && r.is_active() && !r.is_stopped())
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

#[cfg(test)]
mod tests;
