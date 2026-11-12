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

use cockpit_core::api::{AgentStatus, ContainerPruefung, ContainerStatus, ImageBau};
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
    bau: Mutex<ImageBau>,
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
        container::im_pfad(status.laufzeit.as_deref()?, &self.suchpfad())
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
            .filter(|_| container.engine_laeuft)
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
        if !st.bereit {
            return Err(st.fehlt);
        }
        let container = self.container_status();
        let wahl = self.agent_wahl();
        if let Some(m) = wahl.anbieter.nicht_umgesetzt() {
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
            .unwrap_or_else(|| PathBuf::from("agent"))
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
                (!st.bereit).then(|| {
                    format!(
                        "Der Agent kann noch nicht starten. {}",
                        st.fehlt.join(" ")
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
        if let Some(cli) = self.cli_pfad(&status).filter(|_| status.engine_laeuft) {
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
    pub(crate) fn image_stand(&self) -> ImageBau {
        let mut b = sperre(&self.inner.agent.bau).clone();
        let status = self.container_status();
        b.vorhanden = status.engine_laeuft
            && self
                .cli_pfad(&status)
                .is_some_and(|cli| self.image_vorhanden(&cli));
        b
    }

    /// Startet den Bau im Hintergrund. `Err` mit der Erklärung, wenn er nicht starten kann.
    /// Blockierend.
    pub(crate) fn image_bauen(&self) -> Result<ImageBau, String> {
        let status = self.container_status();
        let Some(cli) = self.cli_pfad(&status) else {
            return Err(status.hinweis.unwrap_or_else(|| {
                "Es gibt keine Container-Laufzeit. Installiere Rancher Desktop oder Podman.".into()
            }));
        };
        if !status.engine_laeuft {
            return Err(status.hinweis.unwrap_or_else(|| {
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
            if b.laeuft {
                return Ok(b.clone());
            }
            *b = ImageBau {
                laeuft: true,
                vorhanden: false,
                fehler: None,
                zeilen: vec!["Der Bau startet …".into()],
            };
        }
        let s = self.clone();
        tokio::spawn(async move { bau_aufgabe(s, cli, quelle).await });
        Ok(sperre(&self.inner.agent.bau).clone())
    }

    // ---------- Erreichbarkeit aus dem Container ----------

    /// Prüft, ob ein Container den Endpunkt erreicht (kurzer Lauf des Images im Prüfmodus,
    /// `--rm`), und vergleicht mit dem, was der Mac sieht.
    pub(crate) async fn container_pruefen(&self) -> ContainerPruefung {
        let s = self.clone();
        let vorbereitet = tokio::task::spawn_blocking(move || {
            let status = s.container_status();
            let cli = s.cli_pfad(&status).filter(|_| status.engine_laeuft);
            let image = cli.as_ref().is_some_and(|c| s.image_vorhanden(c));
            (status, cli, image, s.agent_wahl(), s.agent_key())
        })
        .await;
        let Ok((status, cli, image, wahl, key)) = vorbereitet else {
            return ContainerPruefung::default();
        };
        let nicht = |hinweis: &str| ContainerPruefung {
            hinweis: Some(hinweis.into()),
            ..Default::default()
        };
        if let Some(m) = wahl.anbieter.nicht_umgesetzt() {
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
            return nicht(&status.hinweis.unwrap_or_else(|| {
                "Die Container-Laufzeit läuft nicht (Schritt „Container“).".into()
            }));
        };
        if !image {
            return nicht(
                "Das Agent-Image fehlt. Baue es im Schritt „Container“ mit „Image bauen“, dann kann der Container testen.",
            );
        }
        let mac = cockpit_integrations::endpunkt::pruefen(
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
        ContainerPruefung {
            geprueft: true,
            mac_erreicht: mac.erreichbar,
            container_erreicht: erreicht,
            key_ok,
            hinweis: Some(pruefung_hinweis(mac.erreichbar, erreicht, key_ok, &text)),
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
        b.laeuft = false;
        b.vorhanden = fehler.is_none();
        b.fehler = fehler;
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
        b.zeilen.push(z);
        let n = b.zeilen.len();
        if n > MAX_BAU_ZEILEN {
            b.zeilen.drain(..n - MAX_BAU_ZEILEN);
        }
    }
    let status = kind.wait().await;
    match status {
        Ok(s) if s.success() => fertig(None),
        Ok(s) => {
            let zeilen = sperre(&state.inner.agent.bau).zeilen.clone();
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
mod tests {
    use super::*;
    use crate::app::Einstellungen;
    use crate::config::{AgentWahl, EndpunktKonfig};
    use crate::secrets;
    use crate::secrets::{MemoryStore, SecretStore};
    use cockpit_core::api::Anbieter;
    use cockpit_core::{AgentOptions, MessageKind, demo};

    const TOKEN: &str = "sk-test-geheim-123";
    const GITHUB: &str = "ghp_test_geheim_456";

    // ---------- reine Teile ----------

    #[test]
    fn zeitlimit_des_containers() {
        assert_eq!(erlaubte_sekunden(Some(30), 0), 30 * 60 + NACHSPIELZEIT_S);
        assert_eq!(erlaubte_sekunden(Some(30), 10), 40 * 60 + NACHSPIELZEIT_S);
        assert_eq!(
            erlaubte_sekunden(None, 0),
            STANDARD_MAXIMAL_S + NACHSPIELZEIT_S
        );
    }

    #[test]
    fn hinweis_zur_erreichbarkeit_aus_dem_container() {
        let h = pruefung_hinweis(
            true,
            false,
            false,
            "Der Container erreicht den Proxy nicht (ENOTFOUND).",
        );
        assert!(h.contains("Dein Mac erreicht den Proxy, der Container aber nicht"));
        assert!(h.contains("VPN") && h.contains("ENOTFOUND") && h.contains("podman machine"));
        assert!(
            pruefung_hinweis(false, false, false, "x")
                .contains("Weder dein Mac noch der Container")
        );
        assert!(pruefung_hinweis(true, true, false, "Status 401").contains("Key wird abgelehnt"));
        assert!(
            pruefung_hinweis(true, true, true, "").contains("auch aus dem Container erreichbar")
        );
    }

    #[test]
    fn ratschlaege_beim_bauen() {
        let z = |s: &str| vec![s.to_string()];
        assert!(
            bau_hinweis(&z("write: no space left on device"))
                .unwrap()
                .contains("Platz")
        );
        assert!(
            bau_hinweis(&z("dial tcp: lookup registry-1.docker.io: no such host"))
                .unwrap()
                .contains("Netz")
        );
        assert!(bau_hinweis(&z("etwas ganz anderes")).is_none());
    }

    #[tokio::test]
    async fn zeilen_werden_begrenzt_gelesen() {
        let mut daten = Vec::new();
        daten.extend_from_slice(b"{\"t\":\"hinweise_angekommen\"}\n");
        daten.extend(std::iter::repeat_n(b'x', MAX_ZEILE * 3));
        daten.extend_from_slice(b"\nletzte ohne Zeilenende");
        let mut r = BufReader::new(&daten[..]);
        let mut puffer = Vec::new();
        assert!(
            matches!(lies_zeile(&mut r, &mut puffer).await.unwrap(), Some(Gelesen::Zeile(z)) if z.contains("hinweise"))
        );
        assert!(matches!(
            lies_zeile(&mut r, &mut puffer).await.unwrap(),
            Some(Gelesen::ZuGross)
        ));
        assert!(
            matches!(lies_zeile(&mut r, &mut puffer).await.unwrap(), Some(Gelesen::Zeile(z)) if z == "letzte ohne Zeilenende")
        );
        assert!(lies_zeile(&mut r, &mut puffer).await.unwrap().is_none());
        assert!(
            puffer.capacity() < MAX_ZEILE * 2,
            "es wird nie mehr als eine Zeile Maximum gepuffert"
        );
    }

    // ---------- mit Attrappe als docker und lokalem Git-Remote ----------

    /// `docker` als Shell-Skript. Das Szenario eines Laufs steht in `szenario-plan.sh` bzw.
    /// `szenario-lauf.sh` (je nachdem, ob der Klon schreibgeschützt eingebunden ist) und wird
    /// im Skript eingelesen; `$klon`, `$name` und `$d` stehen darin zur Verfügung.
    const DOCKER: &str = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$@" >> "$d/aufrufe.log"
case "$1" in
  info) exit 0 ;;
  image) [ -e "$d/kein-image" ] && exit 1; exit 0 ;;
  build) cat "$d/build.out" 2>/dev/null; [ -e "$d/build.fehler" ] && { echo "write: no space left on device" >&2; exit 1; }; exit 0 ;;
  ps) cat "$d/ps.out" 2>/dev/null; exit 0 ;;
  rm) for a in "$@"; do touch "$d/gestoppt-$a"; done; exit 0 ;;
  pause|unpause) echo "$1" >> "$d/pause.log"; exit 0 ;;
  run)
    shift
    name=""; klon=""; modus="lauf"; prev=""
    for a in "$@"; do
      [ "$prev" = "--name" ] && name="$a"
      case "$a" in
        *:/work:ro) klon="${a%%:/work:ro}"; modus="plan" ;;
        *:/work) klon="${a%%:/work}" ;;
      esac
      prev="$a"
    done
    echo "TOKEN=$ANTHROPIC_AUTH_TOKEN BASE=$ANTHROPIC_BASE_URL MODEL=$ANTHROPIC_MODEL" >> "$d/umgebung.log"
    . "$d/szenario-$modus.sh"
    ;;
esac
exit 0
"#;

    struct Welt {
        tmp: tempfile::TempDir,
        state: AppState,
        secrets: Arc<MemoryStore>,
    }

    fn sh(cwd: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .current_dir(cwd)
            .args(args)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@t.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@t.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    impl Welt {
        fn bin(&self) -> PathBuf {
            self.tmp.path().join("bin")
        }
        fn schreibe(&self, name: &str, inhalt: &str) {
            std::fs::write(self.bin().join(name), inhalt).unwrap();
        }
        fn log(&self, name: &str) -> String {
            std::fs::read_to_string(self.bin().join(name)).unwrap_or_default()
        }
        fn remote(&self) -> PathBuf {
            self.tmp.path().join("git/firma/app.git")
        }
        fn zweige(&self) -> Vec<String> {
            let out = std::process::Command::new("git")
                .current_dir(self.remote())
                .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
                .output()
                .unwrap();
            let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::to_string)
                .collect();
            v.sort();
            v
        }
        fn ws(&self) -> Workspace {
            self.state.inner.live.kopie()
        }
        fn lauf(&self) -> cockpit_core::AgentRun {
            self.ws().run("BAU-142").unwrap()
        }
        async fn aktion(&self, a: Action) -> Result<(), String> {
            self.state
                .aktion_ausfuehren(a)
                .await
                .map(|_| ())
                .map_err(|e| match e {
                    crate::abgleich::AktionFehler::Plan(m) => m,
                    crate::abgleich::AktionFehler::Regel(e) => e.message(),
                    other => format!("{other:?}"),
                })
        }
        async fn start(&self, plan_first: bool) {
            self.aktion(Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions {
                    plan_first,
                    ..AgentOptions::default()
                },
            })
            .await
            .unwrap();
        }
        /// Lässt den Aufpasser arbeiten, bis `bedingung` gilt (höchstens 15 Sekunden).
        async fn warte(&self, was: &str, bedingung: impl Fn(&Workspace) -> bool) {
            let ende = Instant::now() + Duration::from_secs(15);
            loop {
                self.state.aufpassen().await;
                if bedingung(&self.ws()) {
                    return;
                }
                assert!(
                    Instant::now() < ende,
                    "Zeitüberschreitung beim Warten auf: {was}\n{:#?}",
                    self.lauf()
                );
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        }
        async fn warte_auf_ende(&self) {
            let ende = Instant::now() + Duration::from_secs(15);
            while !sperre(&self.state.inner.agent.laufend).is_empty() {
                self.state.aufpassen().await;
                assert!(Instant::now() < ende, "der Lauf endet nicht");
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        }
    }

    /// Eine Welt mit allem eingerichtet. `echt = false` lässt den Container-Teil weg.
    fn welt(eingerichtet: bool) -> Welt {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let docker = bin.join("docker");
        std::fs::write(&docker, DOCKER).unwrap();
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Ein Remote mit `main`.
        let quelle = tmp.path().join("quelle");
        std::fs::create_dir_all(&quelle).unwrap();
        sh(&quelle, &["init", "--quiet", "-b", "main"]);
        std::fs::write(quelle.join("README.md"), "hallo\n").unwrap();
        sh(&quelle, &["add", "-A"]);
        sh(&quelle, &["commit", "--quiet", "-m", "Anfang"]);
        std::fs::create_dir_all(tmp.path().join("git/firma")).unwrap();
        sh(
            tmp.path(),
            &["clone", "--quiet", "--bare", "quelle", "git/firma/app.git"],
        );

        let agent_dir = tmp.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("Containerfile"), "FROM scratch\n").unwrap();

        let secrets = Arc::new(MemoryStore::new());
        if eingerichtet {
            secrets.set(secrets::LITELLM, &Secret::new(TOKEN)).unwrap();
            secrets.set(secrets::GITHUB, &Secret::new(GITHUB)).unwrap();
        }
        let einst = Einstellungen {
            seed: Seed::Leer,
            container_env: Some("docker".into()),
            suchpfad: Some(bin.clone().into_os_string()),
            agent: AgentWahl {
                anbieter: Anbieter::ClaudeAgentSdk,
                endpunkt: eingerichtet.then(|| EndpunktKonfig {
                    url: Some("https://litellm.example".into()),
                    modell: Some("claude-sonnet-5-5".into()),
                }),
            },
            agent_repo: Some("firma/app".into()),
            agent_dir: Some(agent_dir),
            git_basis: Some(format!("file://{}/git", tmp.path().display())),
            daten_dir: Some(tmp.path().join("daten")),
            ..Einstellungen::default()
        };
        let conn = crate::db::open(&tmp.path().join("t.db")).unwrap();
        let state = AppState::neu(
            conn,
            secrets.clone(),
            8787,
            crate::abgleich::Quellen::default(),
            einst,
        );
        state.inner.live.setze_stand_fuer_test(demo::workspace());
        Welt {
            tmp,
            state,
            secrets,
        }
    }

    const BRANCH: &str = "claude/BAU-142-toggle-fokus-ring-fehlt";

    /// Szenario: Der „Container“ ändert eine Datei im Klon, committet und meldet das Ergebnis.
    fn szenario_ok() -> String {
        format!(
            r#"echo neu > "$klon/neu.txt"
git -C "$klon" add -A
git -C "$klon" -c user.name=a -c user.email=a@a.invalid commit -qm Agent
printf '%s\n' \
 '{{"t":"schritt","titel":"Datei ändern","text":"neu.txt","token_ein":1000,"token_aus":200,"ms":1500}}' \
 '{{"t":"dateien","dateien":["neu.txt"]}}' \
 '{{"t":"ergebnis","text":"Fokus-Ring ergänzt.","branch":"{BRANCH}","commit":"abc1234"}}'
exit 0
"#
        )
    }

    #[tokio::test]
    async fn ohne_einrichtung_startet_kein_lauf_und_die_meldung_sagt_wohin() {
        let w = welt(false);
        std::fs::remove_file(w.bin().join("docker")).unwrap();
        let st = w.state.clone();
        let status = tokio::task::spawn_blocking(move || st.agent_status())
            .await
            .unwrap();
        assert_eq!(status.modus, "echt");
        assert!(!status.bereit);
        let text = status.fehlt.join(" ");
        assert!(
            text.contains("Einrichten")
                && text.contains("Container")
                && text.contains("Modell")
                && text.contains("GitHub"),
            "{text}"
        );
        let fehler = w
            .aktion(Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions::default(),
            })
            .await
            .unwrap_err();
        assert!(
            fehler.contains("Der Agent kann noch nicht starten") && fehler.contains("Einrichten"),
            "{fehler}"
        );
        assert!(w.lauf().is_idle(), "es wurde nichts gestartet");
        // Folgeaufträge gibt es noch nicht mit dem echten Agenten.
        let f = w
            .aktion(Action::InvestigateCheck {
                review: 85,
                check: "Lint".into(),
            })
            .await
            .unwrap_err();
        assert!(f.contains("noch nicht mit dem echten Agenten"), "{f}");
    }

    #[tokio::test]
    async fn fehlendes_image_wird_gemeldet() {
        let w = welt(true);
        std::fs::write(w.bin().join("kein-image"), "").unwrap();
        let st = w.state.clone();
        let status = tokio::task::spawn_blocking(move || st.agent_status())
            .await
            .unwrap();
        assert!(!status.bereit && !status.image);
        assert!(
            status.fehlt[0].contains("Image bauen"),
            "{:?}",
            status.fehlt
        );
    }

    #[tokio::test]
    async fn ein_lauf_von_der_freigabe_bis_zum_push() {
        let w = welt(true);
        w.schreibe("szenario-lauf.sh", &szenario_ok());
        w.start(false).await;
        w.warte("Lauf fertig", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_done())
        })
        .await;
        w.warte_auf_ende().await;

        let run = w.lauf();
        assert!(
            run.summary.contains("Branch claude/BAU-142") && run.summary.contains("gepusht"),
            "{}",
            run.summary
        );
        assert_eq!(run.files, ["neu.txt"]);
        // Verbrauch aus dem Lauf, mit den Preisen aus den Einstellungen.
        let u = run.usage();
        assert_eq!(
            (u.input_tokens, u.output_tokens, u.millis),
            (1000, 200, 1500)
        );
        assert!(u.cost_cents >= 1);
        assert_eq!(w.ws().spent_month(jetzt_ms()).input_tokens, 1000);
        // Protokoll: vom Agenten, in Klartext.
        let agent_eintraege: Vec<String> = w
            .ws()
            .log_entries(Some(Actor::Agent))
            .iter()
            .map(|e| e.text.clone())
            .collect();
        assert!(
            agent_eintraege
                .iter()
                .any(|t| t.contains("Container gestartet")),
            "{agent_eintraege:?}"
        );
        assert!(
            agent_eintraege.iter().any(|t| t.contains("Datei ändern")),
            "{agent_eintraege:?}"
        );
        // Der Branch liegt im Remote, main ist unberührt.
        assert_eq!(w.zweige(), [BRANCH, "main"]);
        // Nur CLI-Aufrufe, gemeinsame Schalter, Label je Lauf, Zeitlimit-Name, kein Geheimnis.
        let aufrufe = w.log("aufrufe.log");
        let lauf_zeile = aufrufe
            .lines()
            .find(|z| z.starts_with("run "))
            .expect("run aufgerufen");
        for muss in [
            "--rm",
            "--label cockpit.lauf=BAU-142",
            "--add-host cockpit-host:host-gateway",
            ":/work ",
            ":/auftrag:ro",
            "-e ANTHROPIC_AUTH_TOKEN",
            "localhost/cockpit-agent:1",
        ] {
            assert!(lauf_zeile.contains(muss), "{muss} fehlt: {lauf_zeile}");
        }
        assert!(
            !aufrufe.contains(TOKEN) && !aufrufe.contains(GITHUB),
            "Geheimnis in der Argumentliste: {aufrufe}"
        );
        assert!(
            !aufrufe.contains("https://litellm.example"),
            "auch die Adresse steht nur in der Umgebung"
        );
        // Der Wert kommt über die Umgebung des Kindprozesses an.
        let env = w.log("umgebung.log");
        assert!(
            env.contains(&format!("TOKEN={TOKEN}"))
                && env.contains("BASE=https://litellm.example")
                && env.contains("MODEL=claude-sonnet-5-5"),
            "{env}"
        );
        // Nichts bleibt liegen.
        assert!(
            !w.state
                .agent_ordner()
                .join("laeufe")
                .join("BAU-142")
                .exists()
        );
        // Das Ticket zeigt den Branch, aber es gibt keinen erfundenen Pull Request.
        assert_eq!(
            w.ws().item_by_key("BAU-142").unwrap().branch.as_deref(),
            Some(BRANCH)
        );
    }

    /// Ein Zustand wie `welt`, aber mit anderer Wahl von Anbieter und Endpunkt.
    fn mit_wahl(w: &Welt, wahl: AgentWahl) -> AppState {
        let e = Einstellungen {
            agent: wahl,
            ..w.state.inner.einstellungen.clone()
        };
        let conn = crate::db::open(&w.tmp.path().join("wahl.db")).unwrap();
        let state = AppState::neu(
            conn,
            w.secrets.clone(),
            8787,
            crate::abgleich::Quellen::default(),
            e,
        );
        state.inner.live.setze_stand_fuer_test(demo::workspace());
        state
    }

    #[test]
    fn ein_noch_nicht_umgesetztes_werkzeug_startet_keinen_lauf() {
        let w = welt(true);
        for a in [Anbieter::ClaudeCode, Anbieter::Codex] {
            let state = mit_wahl(
                &w,
                AgentWahl {
                    anbieter: a,
                    endpunkt: w.state.agent_wahl().endpunkt,
                },
            );
            let st = state.agent_status();
            assert!(!st.bereit);
            assert!(
                st.fehlt
                    .iter()
                    .any(|f| f.contains(a.bezeichnung()) && f.contains("noch nicht umgesetzt")),
                "{:?}",
                st.fehlt
            );
            let Err(fehlt) = state.zugang() else {
                panic!("kein Zugang für {a:?}");
            };
            assert!(fehlt[0].contains("noch nicht umgesetzt"));
        }
    }

    #[test]
    fn direkt_beim_anbieter_braucht_nur_key_und_modell() {
        let w = welt(true);
        let state = mit_wahl(
            &w,
            AgentWahl {
                anbieter: Anbieter::ClaudeAgentSdk,
                endpunkt: Some(EndpunktKonfig {
                    url: None,
                    modell: Some("claude-opus-5-5".into()),
                }),
            },
        );
        // Der Key liegt unter dem neuen Namen und geht dem älteren vor.
        w.secrets
            .set(secrets::AGENT_KEY, &Secret::new("sk-ant-neu"))
            .unwrap();
        let st = state.agent_status();
        assert!(st.bereit, "{:?}", st.fehlt);
        let z = state.zugang().unwrap();
        assert_eq!(z.endpunkt_url, None);
        assert_eq!(z.key.expose(), "sk-ant-neu");
        assert_eq!(z.modell, "claude-opus-5-5");

        // Ohne Modell fehlt etwas, und der Hinweis nennt es.
        let state = mit_wahl(&w, AgentWahl::default());
        let st = state.agent_status();
        assert!(!st.bereit);
        assert!(st.fehlt.iter().any(|f| f.contains("der Modellname")));
    }

    #[tokio::test]
    async fn plan_zuerst_dann_freigabe_dann_lauf() {
        let w = welt(true);
        w.schreibe(
            "szenario-plan.sh",
            r#"printf '%s\n' '{"t":"schritt","titel":"Code durchsuchen","text":"","token_ein":500,"token_aus":50,"ms":800}' '{"t":"plan","summary":"Fokus-Ring ergänzen","files":["neu.txt"],"steps":["Lesen","Ändern"]}'
exit 0
"#,
        );
        w.schreibe("szenario-lauf.sh", &szenario_ok());
        w.start(true).await;
        // Solange der Plan erarbeitet wird, lässt er sich nicht freigeben.
        let f = w
            .aktion(Action::ApprovePlan {
                ticket: "BAU-142".into(),
            })
            .await
            .unwrap_err();
        assert!(f.contains("erarbeitet den Plan noch"), "{f}");
        w.warte("Plan liegt vor", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.agent_plan.is_some())
        })
        .await;
        w.warte_auf_ende().await;
        let run = w.lauf();
        assert!(run.is_awaiting());
        assert_eq!(run.plan().unwrap().summary, "Fokus-Ring ergänzen");
        assert_eq!(run.usage().input_tokens, 500, "auch das Planen kostet");
        assert!(
            w.zweige() == ["main"],
            "vor der Freigabe entsteht nichts im Remote"
        );
        // Der Plan-Container bekommt den Klon nur zum Lesen.
        assert!(
            w.log("aufrufe.log")
                .lines()
                .any(|z| z.starts_with("run ") && z.contains(":/work:ro"))
        );
        // Freigabe: jetzt läuft der Container zum Umsetzen.
        w.aktion(Action::ApprovePlan {
            ticket: "BAU-142".into(),
        })
        .await
        .unwrap();
        w.warte("fertig", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_done())
        })
        .await;
        w.warte_auf_ende().await;
        assert_eq!(w.zweige(), [BRANCH, "main"]);
        assert_eq!(
            w.log("aufrufe.log")
                .lines()
                .filter(|z| z.starts_with("run "))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn das_budget_stoppt_den_container() {
        let w = welt(true);
        w.schreibe(
            "szenario-lauf.sh",
            r#"printf '%s\n' '{"t":"schritt","titel":"Viel gelesen","text":"","token_ein":1000000,"token_aus":0,"ms":10}'
while [ ! -e "$d/gestoppt-$name" ]; do sleep 0.05; done
exit 137
"#,
        );
        let mut grenzen = w.ws().limits.clone();
        grenzen.run_budget_cents = Some(50);
        w.aktion(Action::SetAgentLimits { limits: grenzen })
            .await
            .unwrap();
        w.start(false).await;
        w.warte("angehalten", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_stopped())
        })
        .await;
        w.warte_auf_ende().await;
        assert!(
            w.log("aufrufe.log")
                .lines()
                .any(|z| z.starts_with("rm -f cockpit-lauf-BAU-142-")),
            "der Container wurde entfernt"
        );
        let run = w.lauf();
        assert!(
            run.messages
                .iter()
                .any(|m| m.text.starts_with("Angehalten: Budget pro Lauf erreicht"))
        );
        assert_eq!(w.zweige(), ["main"]);
        // Der Klon bleibt für das Fortsetzen.
        assert!(
            w.state
                .agent_ordner()
                .join("laeufe/BAU-142/lauf/.git")
                .exists()
        );
        // Sie wird nicht erneut gestartet, solange sie angehalten ist.
        w.state.aufpassen().await;
        assert!(sperre(&w.state.inner.agent.laufend).is_empty());
    }

    #[tokio::test]
    async fn abbruch_durch_den_nutzer_beendet_den_container_und_raeumt_auf() {
        let w = welt(true);
        w.schreibe(
            "szenario-lauf.sh",
            r#"while [ ! -e "$d/gestoppt-$name" ]; do sleep 0.05; done
exit 137
"#,
        );
        w.start(false).await;
        w.warte("Container läuft", |_| {
            !sperre(&w.state.inner.agent.laufend).is_empty()
                && w.log("aufrufe.log").contains("run ")
        })
        .await;
        w.aktion(Action::CancelAgent {
            ticket: "BAU-142".into(),
        })
        .await
        .unwrap();
        w.warte_auf_ende().await;
        assert!(
            w.log("aufrufe.log")
                .lines()
                .any(|z| z.starts_with("rm -f cockpit-lauf-BAU-142-"))
        );
        let run = w.lauf();
        assert!(run.is_idle());
        assert_eq!(run.ended, None, "ein Abbruch durch dich ist kein Fehler");
        assert!(!w.state.agent_ordner().join("laeufe/BAU-142").exists());
        assert_eq!(w.zweige(), ["main"]);
    }

    #[tokio::test]
    async fn fehler_im_container_stehen_in_klartext_im_lauf_und_ohne_geheimnisse() {
        let w = welt(true);
        // Der Läufer meldet einen Fehler.
        w.schreibe(
            "szenario-lauf.sh",
            r#"printf '%s\n' '{"t":"fehler","text":"Das SDK meldet: Verbindung abgewiesen"}'
exit 1
"#,
        );
        w.start(false).await;
        w.warte("beendet", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
        })
        .await;
        w.warte_auf_ende().await;
        let run = w.lauf();
        assert!(run.is_idle());
        assert_eq!(
            run.ended.as_deref(),
            Some("Das SDK meldet: Verbindung abgewiesen")
        );
        assert!(run.log().contains("Verbindung abgewiesen"));
        assert!(!w.state.agent_ordner().join("laeufe/BAU-142").exists());

        // Der Container stirbt ohne Meldung; seine Fehlerausgabe enthält ein Geheimnis.
        w.schreibe(
            "szenario-lauf.sh",
            &format!("echo \"boom mit {TOKEN} und {GITHUB}\" >&2\nexit 3\n"),
        );
        sperre(&w.state.inner.agent.gescheitert).clear();
        w.start(false).await;
        w.warte("beendet", |ws| {
            ws.run("BAU-142").is_some_and(|r| {
                r.ended
                    .as_deref()
                    .is_some_and(|e| e.contains("Fehlercode 3"))
            })
        })
        .await;
        let ended = w.lauf().ended.unwrap();
        assert!(ended.contains("boom mit"), "{ended}");
        assert!(!ended.contains(TOKEN) && !ended.contains(GITHUB), "{ended}");
        assert!(
            !w.ws()
                .log
                .iter()
                .any(|e| e.text.contains(TOKEN) || e.text.contains(GITHUB))
        );
    }

    #[tokio::test]
    async fn docker_startet_nicht_in_klartext() {
        let w = welt(true);
        w.schreibe(
            "szenario-lauf.sh",
            "echo 'Cannot connect to the Docker daemon' >&2\nexit 125\n",
        );
        w.start(false).await;
        w.warte("beendet", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
        })
        .await;
        let e = w.lauf().ended.unwrap();
        assert!(
            e.contains("ließ sich nicht starten") && e.contains("Docker daemon"),
            "{e}"
        );
    }

    #[tokio::test]
    async fn unbrauchbare_zeilen_werden_verworfen_der_rest_gilt() {
        let w = welt(true);
        let mut s = String::from(
            "echo '{{{ kaputt'\necho 'irgendein Text'\necho '{\"t\":\"unbekannt\"}'\necho '{\"t\":\"schritt\"}'\n",
        );
        s.push_str(&format!(
            "head -c 100000 /dev/zero | tr '\\0' x; echo\n{}",
            szenario_ok()
        ));
        w.schreibe("szenario-lauf.sh", &s);
        w.start(false).await;
        w.warte("fertig", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_done())
        })
        .await;
        assert_eq!(w.lauf().usage().input_tokens, 1000);
    }

    #[tokio::test]
    async fn rueckfrage_antwort_und_hinweis_gehen_ueber_stdin() {
        let w = welt(true);
        w.schreibe(
            "szenario-lauf.sh",
            &format!(
                r#"printf '%s\n' '{{"t":"frage","text":"Welcher Farbton?","vorschlaege":["blau","rot"]}}'
read -r antwort
echo "$antwort" >> "$d/stdin.log"
read -r hinweis
echo "$hinweis" >> "$d/stdin.log"
printf '%s\n' '{{"t":"hinweise_angekommen"}}'
{}"#,
                szenario_ok()
            ),
        );
        w.start(false).await;
        w.warte("Frage", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_asking())
        })
        .await;
        assert_eq!(w.lauf().question.unwrap().suggestions, ["blau", "rot"]);
        w.aktion(Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "blau".into(),
        })
        .await
        .unwrap();
        w.warte("Antwort angekommen", |_| {
            w.log("stdin.log").contains("antwort")
        })
        .await;
        assert!(
            w.log("stdin.log")
                .contains(r#"{"t":"antwort","text":"blau"}"#),
            "{}",
            w.log("stdin.log")
        );
        w.aktion(Action::SteerAgent {
            run: "BAU-142".into(),
            text: "Kleine Commits".into(),
        })
        .await
        .unwrap();
        w.warte("fertig", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_done())
        })
        .await;
        assert!(
            w.log("stdin.log")
                .contains(r#"{"t":"hinweis","text":"Kleine Commits"}"#)
        );
        let run = w.lauf();
        assert!(run.pending_hints.is_empty());
        assert!(
            run.messages
                .iter()
                .any(|m| m.kind == MessageKind::Ack && m.text.contains("Kleine Commits"))
        );
    }

    #[tokio::test]
    async fn pause_und_fortsetzen_gehen_an_den_container() {
        let w = welt(true);
        w.schreibe(
            "szenario-lauf.sh",
            &format!(
                "while [ ! -e \"$d/weiter\" ]; do sleep 0.05; done\n{}",
                szenario_ok()
            ),
        );
        w.start(false).await;
        w.warte("Container", |_| w.log("aufrufe.log").contains("run "))
            .await;
        w.aktion(Action::PauseAgent {
            run: "BAU-142".into(),
        })
        .await
        .unwrap();
        w.warte("pausiert", |_| w.log("pause.log").contains("pause"))
            .await;
        w.aktion(Action::ResumeAgent {
            run: "BAU-142".into(),
        })
        .await
        .unwrap();
        w.warte("fortgesetzt", |_| w.log("pause.log").contains("unpause"))
            .await;
        w.schreibe("weiter", "");
        w.warte("fertig", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_done())
        })
        .await;
    }

    #[tokio::test]
    async fn zeitlimit_beendet_den_container_mit_klartext() {
        let mut w = welt(true);
        // Eigene Einstellungen mit 1 Sekunde Obergrenze.
        let e = Einstellungen {
            lauf_sekunden: Some(1),
            ..w.state.inner.einstellungen.clone()
        };
        let conn = crate::db::open(&w.tmp.path().join("z.db")).unwrap();
        w.state = AppState::neu(
            conn,
            w.secrets.clone(),
            8787,
            crate::abgleich::Quellen::default(),
            e,
        );
        w.state.inner.live.setze_stand_fuer_test(demo::workspace());
        w.schreibe(
            "szenario-lauf.sh",
            "while [ ! -e \"$d/gestoppt-$name\" ]; do sleep 0.05; done\nexit 137\n",
        );
        w.start(false).await;
        w.warte("Zeitlimit", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
        })
        .await;
        let e = w.lauf().ended.unwrap();
        assert!(e.contains("Zeitlimit") && e.contains("beendet"), "{e}");
        assert!(
            w.log("aufrufe.log")
                .lines()
                .any(|z| z.starts_with("rm -f cockpit-lauf-BAU-142-"))
        );
    }

    #[tokio::test]
    async fn verbotene_pfade_verhindern_den_push() {
        let w = welt(true);
        let r = cockpit_core::RepoRules {
            forbidden_paths: vec!["*.txt".into()],
            ..Default::default()
        };
        w.aktion(Action::SetRepoRules {
            repo: cockpit_core::DEMO_REPO.into(),
            rules: r,
        })
        .await
        .unwrap();
        w.schreibe("szenario-lauf.sh", &szenario_ok());
        w.start(false).await;
        w.warte("beendet", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
        })
        .await;
        assert!(
            w.lauf()
                .ended
                .unwrap()
                .contains("verstoßen gegen die Regeln")
        );
        assert_eq!(w.zweige(), ["main"]);
    }

    #[tokio::test]
    async fn neustart_raeumt_verwaiste_container_auf_und_markiert_laeufe() {
        let w = welt(true);
        w.schreibe(
            "szenario-lauf.sh",
            "while [ ! -e \"$d/gestoppt-$name\" ]; do sleep 0.05; done\nexit 137\n",
        );
        w.start(false).await;
        w.warte("Container", |_| w.log("aufrufe.log").contains("run "))
            .await;
        // „Neustart“: neuer Server-Zustand mit demselben gespeicherten Stand, Container sind weg.
        let ws = w.ws();
        let conn = crate::db::open(&w.tmp.path().join("neu.db")).unwrap();
        let neu = AppState::neu(
            conn,
            w.secrets.clone(),
            8787,
            crate::abgleich::Quellen::default(),
            w.state.inner.einstellungen.clone(),
        );
        neu.inner.live.setze_stand_fuer_test(ws);
        w.schreibe("ps.out", "abc123\ndef456\n");
        let n = neu.clone();
        tokio::task::spawn_blocking(move || n.agent_aufraeumen())
            .await
            .unwrap();
        let aufrufe = w.log("aufrufe.log");
        assert!(
            aufrufe.contains("ps -a -q --filter label=cockpit.lauf"),
            "{aufrufe}"
        );
        assert!(aufrufe.contains("rm -f abc123 def456"), "{aufrufe}");
        let run = neu.inner.live.kopie().run("BAU-142").unwrap();
        assert!(run.is_idle());
        assert!(run.ended.unwrap().contains("Server wurde neu gestartet"));
        // Den alten Lauf im ersten Zustand beenden.
        w.aktion(Action::CancelAgent {
            ticket: "BAU-142".into(),
        })
        .await
        .unwrap();
        w.warte_auf_ende().await;
    }

    #[tokio::test]
    async fn bau_des_images_mit_fortschritt_und_fehler() {
        let w = welt(true);
        w.schreibe("build.out", "Step 1/5 : FROM docker.io/library/node:24-bookworm-slim\nStep 2/5 : RUN apt-get update\n");
        let st = w.state.clone();
        let b = tokio::task::spawn_blocking(move || st.image_bauen())
            .await
            .unwrap()
            .unwrap();
        assert!(b.laeuft);
        let ende = Instant::now() + Duration::from_secs(10);
        loop {
            let st = w.state.clone();
            let b = tokio::task::spawn_blocking(move || st.image_stand())
                .await
                .unwrap();
            if !b.laeuft {
                assert!(b.fehler.is_none(), "{b:?}");
                assert!(b.vorhanden);
                assert!(
                    b.zeilen.iter().any(|z| z.starts_with("Step 2/5")),
                    "{:?}",
                    b.zeilen
                );
                break;
            }
            assert!(Instant::now() < ende);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let aufrufe = w.log("aufrufe.log");
        let bau = aufrufe.lines().find(|z| z.starts_with("build ")).unwrap();
        assert!(
            bau.contains("-f ")
                && bau.contains("Containerfile")
                && bau.contains("-t localhost/cockpit-agent:1"),
            "{bau}"
        );

        // Fehlschlag mit Rat.
        w.schreibe("build.fehler", "");
        let st = w.state.clone();
        tokio::task::spawn_blocking(move || st.image_bauen())
            .await
            .unwrap()
            .unwrap();
        let ende = Instant::now() + Duration::from_secs(10);
        loop {
            let st = w.state.clone();
            let b = tokio::task::spawn_blocking(move || st.image_stand())
                .await
                .unwrap();
            if !b.laeuft {
                let f = b.fehler.expect("Fehler");
                assert!(f.contains("gescheitert") && f.contains("Platz"), "{f}");
                break;
            }
            assert!(Instant::now() < ende);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn bau_ohne_containerfile_und_ohne_laufzeit_sagt_warum() {
        let w = welt(true);
        std::fs::remove_file(
            w.state
                .inner
                .einstellungen
                .agent_dir
                .clone()
                .unwrap()
                .join("Containerfile"),
        )
        .unwrap();
        let st = w.state.clone();
        let e = tokio::task::spawn_blocking(move || st.image_bauen())
            .await
            .unwrap()
            .unwrap_err();
        assert!(e.contains("Containerfile fehlt"), "{e}");
        // Ohne Container-Programm: neuer Zustand (der Zwischenspeicher gilt 10 Sekunden).
        let w = welt(true);
        std::fs::remove_file(w.bin().join("docker")).unwrap();
        let st = w.state.clone();
        let e = tokio::task::spawn_blocking(move || st.image_bauen())
            .await
            .unwrap()
            .unwrap_err();
        assert!(e.contains("docker") && e.contains("PATH"), "{e}");
    }

    #[tokio::test]
    async fn pruefung_aus_dem_container_nennt_vpn_wenn_nur_der_mac_den_proxy_erreicht() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let proxy = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":[{"id":"claude-sonnet-5-5"}]})),
            )
            .mount(&proxy)
            .await;
        let w = welt(true);
        // Der Proxy des Tests liegt auf dem Mac; der „Container“ erreicht ihn nicht.
        let e = Einstellungen {
            agent: AgentWahl {
                anbieter: Anbieter::ClaudeAgentSdk,
                endpunkt: Some(EndpunktKonfig {
                    url: Some(proxy.uri()),
                    modell: Some("claude-sonnet-5-5".into()),
                }),
            },
            ..w.state.inner.einstellungen.clone()
        };
        let conn = crate::db::open(&w.tmp.path().join("p.db")).unwrap();
        let state = AppState::neu(
            conn,
            w.secrets.clone(),
            8787,
            crate::abgleich::Quellen::default(),
            e,
        );
        w.schreibe(
            "szenario-plan.sh",
            r#"printf '%s\n' '{"t":"pruefung","erreichbar":false,"key_ok":false,"hinweis":"Der Container erreicht den Proxy nicht (ENOTFOUND)."}'
exit 1
"#,
        );
        let p = state.container_pruefen().await;
        assert!(
            p.geprueft && p.mac_erreicht && !p.container_erreicht,
            "{p:?}"
        );
        let h = p.hinweis.unwrap();
        assert!(
            h.contains("Dein Mac erreicht den Proxy, der Container aber nicht")
                && h.contains("VPN")
                && h.contains("ENOTFOUND"),
            "{h}"
        );
        let aufrufe = w.log("aufrufe.log");
        let z = aufrufe.lines().find(|z| z.starts_with("run ")).unwrap();
        assert!(z.contains("--rm") && z.contains(":ro"), "{z}");
        assert!(!aufrufe.contains(TOKEN));
        // Läuft er auch im Container, ist alles gut.
        w.schreibe(
            "szenario-plan.sh",
            "printf '%s\\n' '{\"t\":\"pruefung\",\"erreichbar\":true,\"key_ok\":true,\"hinweis\":\"ok\"}'\nexit 0\n",
        );
        let p = state.container_pruefen().await;
        assert!(p.container_erreicht && p.key_ok);
        assert!(
            p.hinweis
                .unwrap()
                .contains("auch aus dem Container erreichbar")
        );
    }

    #[tokio::test]
    async fn pruefung_aus_dem_container_ohne_image_oder_key_ruft_nichts_auf() {
        let w = welt(false);
        let p = w.state.container_pruefen().await;
        assert!(!p.geprueft && p.hinweis.unwrap().contains("nicht eingerichtet"));
        let w = welt(true);
        std::fs::write(w.bin().join("kein-image"), "").unwrap();
        let p = w.state.container_pruefen().await;
        assert!(!p.geprueft && p.hinweis.unwrap().contains("Image fehlt"));
        assert!(!w.log("aufrufe.log").contains("run "));
    }
}
