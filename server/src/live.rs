//! Der Workspace des Server-Modus: hält den Stand im Speicher, wendet Aktionen an,
//! sichert ihn in SQLite und meldet jede Änderung an die offenen Ereignis-Verbindungen.
//!
//! Der Server kennt die einzelnen [`Action`]-Varianten nicht. Er reicht jede Aktion an
//! `Workspace::apply` weiter – neue Varianten im Kern laufen deshalb ohne Änderung hier.

use std::sync::Mutex;

use cockpit_core::api::{ActionResponse, ServerEvent};
use cockpit_core::{Action, ActionError, Actor, Workspace, demo, persist};
use rusqlite::Connection;
use tokio::sync::broadcast;

use crate::config::Seed;
use crate::db;

/// So viele Ereignisse puffert eine langsame Verbindung, bevor sie neu laden muss.
const PUFFER: usize = 64;

pub struct Live {
    stand: Mutex<Stand>,
    tx: broadcast::Sender<ServerEvent>,
}

struct Stand {
    ws: Workspace,
    revision: u64,
}

/// Warum eine Aktion nicht angewendet wurde.
#[derive(Debug)]
pub enum Fehler {
    /// Die Regeln des Workspace lehnen sie ab (der Stand bleibt unverändert).
    Aktion(ActionError),
    /// Der neue Stand ließ sich nicht sichern. Dann wird er auch nicht übernommen.
    Speichern(String),
}

fn sperre<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Ein abgestürzter Handler soll nicht alle weiteren lahmlegen.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Live {
    /// Lädt den gespeicherten Stand; ohne (oder bei unlesbarem) Stand beginnt der Server mit
    /// dem Seed: standardmäßig ein leerer Workspace, mit `Seed::Demo` die Beispieldaten.
    /// Ein gespeicherter Stand geht dem Seed immer vor.
    pub fn laden(conn: &Connection, seed: Seed) -> Self {
        let anfang = || match seed {
            Seed::Leer => Workspace::default(),
            Seed::Demo => demo::workspace(),
        };
        let gespeichert = db::workspace_laden(conn).unwrap_or_else(|e| {
            tracing::warn!("Gespeicherter Workspace nicht lesbar: {e}");
            None
        });
        let (ws, revision) = match gespeichert.as_deref().map(persist::load_with_revision) {
            Some(Ok(stand)) => stand,
            Some(Err(e)) => {
                tracing::warn!(
                    "Gespeicherter Workspace passt nicht ({e:?}), starte mit dem Seed ({seed:?})"
                );
                (anfang(), 0)
            }
            None => (anfang(), 0),
        };
        Self {
            stand: Mutex::new(Stand { ws, revision }),
            tx: broadcast::channel(PUFFER).0,
        }
    }

    /// Der aktuelle Stand als JSON (Form von `persist`, plus `revision`).
    pub fn schnappschuss(&self) -> String {
        let stand = sperre(&self.stand);
        persist::save_with_revision(&stand.ws, stand.revision)
    }

    /// Eine Kopie des aktuellen Stands (für Proben und den Abgleich).
    pub fn kopie(&self) -> Workspace {
        sperre(&self.stand).ws.clone()
    }

    pub fn revision(&self) -> u64 {
        sperre(&self.stand).revision
    }

    /// Ersetzt den Stand (nur für Tests, die Daten brauchen, die keine Aktion herstellt).
    #[cfg(test)]
    pub fn setze_stand_fuer_test(&self, ws: Workspace) {
        sperre(&self.stand).ws = ws;
    }

    /// Wendet eine Aktion an, sichert den neuen Stand und meldet ihn. Blockierend (SQLite).
    pub fn anwenden(
        &self,
        db: &Mutex<Connection>,
        action: Action,
        actor: Actor,
        at_ms: u64,
    ) -> Result<ActionResponse, Fehler> {
        let mut stand = sperre(&self.stand);
        // Auf einer Kopie arbeiten: Scheitert die Regel oder das Sichern, bleibt alles wie es war.
        let mut neu = stand.ws.clone();
        let angewendet = neu.apply(action, actor, at_ms).map_err(Fehler::Aktion)?;
        let revision = stand.revision + 1;
        let text = persist::save_with_revision(&neu, revision);
        db::workspace_speichern(&sperre(db), revision, &text)
            .map_err(|e| Fehler::Speichern(e.to_string()))?;

        let neue = angewendet.logged.min(neu.log.len());
        let eintraege = neu.log[neu.log.len() - neue..].to_vec();
        stand.ws = neu;
        stand.revision = revision;
        // Niemand verbunden ist kein Fehler.
        let _ = self.tx.send(ServerEvent {
            revision,
            eintraege,
        });
        Ok(ActionResponse {
            revision,
            zusammenfassung: angewendet.summary,
            protokolliert: angewendet.logged,
        })
    }

    /// Der Takt: Jeder simulierte Agent (nur mit `agenten_simulieren`; echte Läufe melden sich selbst)
    /// und jeder neu gestartete Check macht einen Schritt,
    /// und abgelaufene Zurückstellungen kommen zurück in die Inbox, Pull Requests mit Ticket-Schlüssel
    /// in Branch oder Titel werden verknüpft, und der Status verknüpfter Tickets folgt dem Pull Request. Ist nichts zu tun, passiert nichts
    /// (und es gibt keine Revision).
    pub fn takt(&self, db: &Mutex<Connection>, at_ms: u64, agenten_simulieren: bool) {
        let (agenten, checks, wecken, verknuepfen, status) = {
            let stand = sperre(&self.stand);
            (
                stand.ws.running_agents() > 0,
                stand.ws.running_checks() > 0,
                !stand.ws.expired_snoozes(at_ms).is_empty(),
                !stand.ws.auto_links().is_empty(),
                stand.ws.status_pending(),
            )
        };
        let mut schritte = Vec::new();
        if agenten && agenten_simulieren {
            schritte.push((Action::AdvanceAgents, Actor::Agent));
        }
        if checks {
            schritte.push((Action::AdvanceChecks, Actor::System));
        }
        if wecken {
            schritte.push((Action::WakeSnoozed, Actor::System));
        }
        if verknuepfen {
            schritte.push((Action::AutoLinkPrs, Actor::System));
        }
        if status {
            schritte.push((Action::SyncTicketStatus, Actor::System));
        }
        for (action, actor) in schritte {
            match self.anwenden(db, action, actor, at_ms) {
                Ok(_) | Err(Fehler::Aktion(_)) => {}
                Err(Fehler::Speichern(m)) => tracing::warn!("Takt nicht gesichert: {m}"),
            }
        }
    }

    /// Meldet an: der Empfänger für kommende Ereignisse und das erste Ereignis mit der
    /// aktuellen Revision (ohne Einträge). Beides unter derselben Sperre, damit keine
    /// Änderung dazwischen verloren geht.
    pub fn abonnieren(&self) -> (broadcast::Receiver<ServerEvent>, ServerEvent) {
        let stand = sperre(&self.stand);
        let rx = self.tx.subscribe();
        let erstes = ServerEvent {
            revision: stand.revision,
            eintraege: Vec::new(),
        };
        (rx, erstes)
    }
}
