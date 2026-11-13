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
    snapshot: Mutex<Snapshot>,
    tx: broadcast::Sender<ServerEvent>,
}

struct Snapshot {
    ws: Workspace,
    revision: u64,
}

/// Warum eine Aktion nicht angewendet wurde.
#[derive(Debug)]
pub enum Error {
    /// Die Regeln des Workspace lehnen sie ab (der Stand bleibt unverändert).
    Action(ActionError),
    /// Der neue Stand ließ sich nicht sichern. Dann wird er auch nicht übernommen.
    Save(String),
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Ein abgestürzter Handler soll nicht alle weiteren lahmlegen.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Live {
    /// Lädt den gespeicherten Stand; ohne (oder bei unlesbarem) Stand beginnt der Server mit
    /// dem Seed: standardmäßig ein leerer Workspace, mit `Seed::Demo` die Beispieldaten.
    /// Ein gespeicherter Stand geht dem Seed immer vor.
    pub fn laden(conn: &Connection, seed: Seed) -> Self {
        let start = || match seed {
            Seed::Empty => Workspace::default(),
            Seed::Demo => demo::workspace(),
        };
        let stored = db::workspace_laden(conn).unwrap_or_else(|e| {
            tracing::warn!("Gespeicherter Workspace nicht lesbar: {e}");
            None
        });
        let (ws, revision) = match stored.as_deref().map(persist::load_with_revision) {
            Some(Ok(snapshot)) => snapshot,
            Some(Err(e)) => {
                tracing::warn!(
                    "Gespeicherter Workspace passt nicht ({e:?}), starte mit dem Seed ({seed:?})"
                );
                (start(), 0)
            }
            None => (start(), 0),
        };
        Self {
            snapshot: Mutex::new(Snapshot { ws, revision }),
            tx: broadcast::channel(PUFFER).0,
        }
    }

    /// Der aktuelle Stand als JSON (Form von `persist`, plus `revision`).
    pub fn snapshot(&self) -> String {
        let snapshot = lock(&self.snapshot);
        persist::save_with_revision(&snapshot.ws, snapshot.revision)
    }

    /// Eine Kopie des aktuellen Stands (für Proben und den Abgleich).
    pub fn copy(&self) -> Workspace {
        lock(&self.snapshot).ws.clone()
    }

    pub fn revision(&self) -> u64 {
        lock(&self.snapshot).revision
    }

    /// Ersetzt den Stand (nur für Tests, die Daten brauchen, die keine Aktion herstellt).
    #[cfg(test)]
    pub fn set_state_for_test(&self, ws: Workspace) {
        lock(&self.snapshot).ws = ws;
    }

    /// Wendet eine Aktion an, sichert den neuen Stand und meldet ihn. Blockierend (SQLite).
    pub fn apply(
        &self,
        db: &Mutex<Connection>,
        action: Action,
        actor: Actor,
        at_ms: u64,
    ) -> Result<ActionResponse, Error> {
        let mut snapshot = lock(&self.snapshot);
        // Auf einer Kopie arbeiten: Scheitert die Regel oder das Sichern, bleibt alles wie es war.
        let mut updated = snapshot.ws.clone();
        let applied = updated.apply(action, actor, at_ms).map_err(Error::Action)?;
        let revision = snapshot.revision + 1;
        let text = persist::save_with_revision(&updated, revision);
        db::workspace_save(&lock(db), revision, &text).map_err(|e| Error::Save(e.to_string()))?;

        let added = applied.logged.min(updated.log.len());
        let entries = updated.log[updated.log.len() - added..].to_vec();
        snapshot.ws = updated;
        snapshot.revision = revision;
        // Niemand verbunden ist kein Fehler.
        let _ = self.tx.send(ServerEvent { revision, entries });
        Ok(ActionResponse {
            revision,
            summary: applied.summary,
            logged: applied.logged,
        })
    }

    /// Der Takt: Jeder simulierte Agent (nur mit `agenten_simulieren`; echte Läufe melden sich selbst)
    /// und jeder neu gestartete Check macht einen Schritt,
    /// und abgelaufene Zurückstellungen kommen zurück in die Inbox, Pull Requests mit Ticket-Schlüssel
    /// in Branch oder Titel werden verknüpft, und der Status verknüpfter Tickets folgt dem Pull Request. Ist nichts zu tun, passiert nichts
    /// (und es gibt keine Revision).
    pub fn takt(&self, db: &Mutex<Connection>, at_ms: u64, agents_simulate: bool) {
        let (agents, checks, wake, link, status) = {
            let snapshot = lock(&self.snapshot);
            (
                snapshot.ws.running_agents() > 0,
                snapshot.ws.running_checks() > 0,
                !snapshot.ws.expired_snoozes(at_ms).is_empty(),
                !snapshot.ws.auto_links().is_empty(),
                snapshot.ws.status_pending(),
            )
        };
        let mut steps = Vec::new();
        if agents && agents_simulate {
            steps.push((Action::AdvanceAgents, Actor::Agent));
        }
        if checks {
            steps.push((Action::AdvanceChecks, Actor::System));
        }
        if wake {
            steps.push((Action::WakeSnoozed, Actor::System));
        }
        if link {
            steps.push((Action::AutoLinkPrs, Actor::System));
        }
        if status {
            steps.push((Action::SyncTicketStatus, Actor::System));
        }
        for (action, actor) in steps {
            match self.apply(db, action, actor, at_ms) {
                Ok(_) | Err(Error::Action(_)) => {}
                Err(Error::Save(m)) => tracing::warn!("Takt nicht gesichert: {m}"),
            }
        }
    }

    /// Meldet an: der Empfänger für kommende Ereignisse und das erste Ereignis mit der
    /// aktuellen Revision (ohne Einträge). Beides unter derselben Sperre, damit keine
    /// Änderung dazwischen verloren geht.
    pub fn subscribe(&self) -> (broadcast::Receiver<ServerEvent>, ServerEvent) {
        let snapshot = lock(&self.snapshot);
        let rx = self.tx.subscribe();
        let first = ServerEvent {
            revision: snapshot.revision,
            entries: Vec::new(),
        };
        (rx, first)
    }
}
