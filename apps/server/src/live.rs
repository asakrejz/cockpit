//! The workspace of the server mode: holds the state in memory, applies actions,
//! saves it in SQLite and reports every change to the open event connections.
//!
//! The server does not know the individual [`Action`] variants. It passes every action on to
//! `Workspace::apply` – new variants in the core therefore work here without change.

use std::sync::Mutex;

use cockpit_core::api::{ActionResponse, ServerEvent};
use cockpit_core::{Action, ActionError, Actor, Workspace, demo, persist};
use rusqlite::Connection;
use tokio::sync::broadcast;

use crate::config::Seed;
use crate::db;

/// This many events a slow connection buffers before it has to reload.
const PUFFER: usize = 64;

pub struct Live {
    snapshot: Mutex<Snapshot>,
    tx: broadcast::Sender<ServerEvent>,
}

struct Snapshot {
    ws: Workspace,
    revision: u64,
}

/// Why an action was not applied.
#[derive(Debug)]
pub enum Error {
    /// The rules of the workspace reject it (the state stays unchanged).
    Action(ActionError),
    /// The new state could not be saved. Then it is not taken over either.
    Save(String),
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A crashed handler should not paralyze all others.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Live {
    /// Loads the stored state; without (or with an unreadable) state the server starts with
    /// the seed: by default an empty workspace, with `Seed::Demo` the sample data.
    /// A stored state always goes before the seed.
    pub fn load(conn: &Connection, seed: Seed) -> Self {
        let start = || match seed {
            Seed::Empty => Workspace::default(),
            Seed::Demo => demo::multi_repo_workspace(),
        };
        let stored = db::load_workspace(conn).unwrap_or_else(|e| {
            tracing::warn!("Stored workspace not readable: {e}");
            None
        });
        let (ws, revision) = match stored.as_deref().map(persist::load_with_revision) {
            Some(Ok(snapshot)) => snapshot,
            Some(Err(e)) => {
                tracing::warn!(
                    "Stored workspace does not fit ({e:?}), starting with the seed ({seed:?})"
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

    /// Fills the repo of pull requests and runs of a state stored before several repos were
    /// supported (see [`Workspace::fill_repos`]); `only_repo` is the one configured repo, if
    /// there is exactly one. Saves the result if anything changed.
    pub fn fill_repos(&self, db: &Connection, only_repo: Option<&str>) {
        let mut snapshot = lock(&self.snapshot);
        let mut updated = snapshot.ws.clone();
        if !updated.fill_repos(only_repo) {
            return;
        }
        let revision = snapshot.revision + 1;
        let text = persist::save_with_revision(&updated, revision);
        match db::workspace_save(db, revision, &text) {
            Ok(()) => {
                snapshot.ws = updated;
                snapshot.revision = revision;
            }
            Err(e) => tracing::warn!("Repos des alten Stands nicht gesichert: {e}"),
        }
    }

    /// The current state as JSON (shape of `persist`, plus `revision`).
    pub fn snapshot(&self) -> String {
        let snapshot = lock(&self.snapshot);
        persist::save_with_revision(&snapshot.ws, snapshot.revision)
    }

    /// A copy of the current state (for probes and the sync).
    pub fn copy(&self) -> Workspace {
        lock(&self.snapshot).ws.clone()
    }

    pub fn revision(&self) -> u64 {
        lock(&self.snapshot).revision
    }

    /// Replaces the state (for tests only that need data that no action produces).
    #[cfg(test)]
    pub fn set_state_for_test(&self, ws: Workspace) {
        lock(&self.snapshot).ws = ws;
    }

    /// Applies an action, saves the new state and reports it. Blocking (SQLite).
    pub fn apply(
        &self,
        db: &Mutex<Connection>,
        action: Action,
        actor: Actor,
        at_ms: u64,
    ) -> Result<ActionResponse, Error> {
        let mut snapshot = lock(&self.snapshot);
        // Work on a copy: if the rule or the saving fails, everything stays as it was.
        let mut updated = snapshot.ws.clone();
        let applied = updated.apply(action, actor, at_ms).map_err(Error::Action)?;
        let revision = snapshot.revision + 1;
        let text = persist::save_with_revision(&updated, revision);
        db::workspace_save(&lock(db), revision, &text).map_err(|e| Error::Save(e.to_string()))?;

        let added = applied.logged.min(updated.log.len());
        let entries = updated.log[updated.log.len() - added..].to_vec();
        snapshot.ws = updated;
        snapshot.revision = revision;
        // Nobody connected is no error.
        let _ = self.tx.send(ServerEvent { revision, entries });
        Ok(ActionResponse {
            revision,
            summary: applied.summary,
            logged: applied.logged,
        })
    }

    /// The tick: every simulated agent (only with `simulate_agents`; real runs report themselves)
    /// and every newly started check takes a step,
    /// and expired snoozes come back to the inbox, pull requests with a ticket key
    /// in branch or title are linked, and the status of linked tickets follows the pull request. If there is nothing to do, nothing happens
    /// (and there is no revision).
    pub fn tick(&self, db: &Mutex<Connection>, at_ms: u64, agents_simulate: bool) {
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
                Err(Error::Save(m)) => tracing::warn!("Tick not saved: {m}"),
            }
        }
    }

    /// Subscribes: the receiver for coming events and the first event with the
    /// current revision (without entries). Both under the same lock so that no
    /// change gets lost in between.
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
