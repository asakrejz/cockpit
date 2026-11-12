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
}

mod container_check;
mod image_build;
mod lines;
mod run;
mod watcher;

pub use container_check::pruefung_hinweis;
use container_check::{
    cli_still, container_entfernen, container_labels_entfernen, kind_umgebung, letzte_zeile,
};
pub use image_build::bau_hinweis;
pub use lines::{Gelesen, lies_zeile};
use run::lauf_aufgabe;
pub use watcher::aufpasser_starten;

#[cfg(test)]
mod tests;
