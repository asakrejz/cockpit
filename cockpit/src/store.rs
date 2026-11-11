//! Gemeinsamer Zustand der App. Liegt als Kontext im Baum; `Store` ist `Copy`,
//! lässt sich also frei in Closures verwenden.

use std::time::Duration;

use cockpit_core::{Connections, Filter, Workspace, demo, persist};
use leptos::prelude::*;

use crate::route::Route;

/// Unter diesem Namen liegen die Verbindungs-Haken im Browser. Schlüssel nie.
const STORAGE_KEY: &str = "cockpit.connections";
/// Hier liegt der Workspace (JSON mit Versionsnummer, siehe `cockpit_core::persist`).
/// Auch hier gilt: nur Daten des Cockpits, nie Schlüssel.
const WORKSPACE_KEY: &str = "cockpit.workspace";
/// Änderungen innerhalb dieser Zeit werden zu einem Schreibvorgang zusammengefasst.
const SAVE_DELAY: Duration = Duration::from_millis(400);

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

#[derive(Clone, Copy)]
pub struct Store {
    pub ws: RwSignal<Workspace>,
    pub conn: RwSignal<Connections>,
    /// ID des Eintrags, der in der Inbox gewählt ist.
    pub selected: RwSignal<String>,
    pub route: RwSignal<Route>,
}

impl Store {
    pub fn new() -> Self {
        let saved = storage().and_then(|s| s.get_item(STORAGE_KEY).ok().flatten());
        // Gespeicherter Stand, sonst (auch bei alter Version oder kaputten Daten) die Beispieldaten.
        let saved_ws = storage().and_then(|s| s.get_item(WORKSPACE_KEY).ok().flatten());
        let (ws, _) = persist::restore(saved_ws.as_deref());
        let store = Store {
            selected: RwSignal::new(first_id(&ws)),
            ws: RwSignal::new(ws),
            conn: RwSignal::new(saved.map(|s| Connections::decode(&s)).unwrap_or_default()),
            route: RwSignal::new(Route::parse(&current_hash())),
        };
        // Jede Änderung der Verbindungen wird gesichert.
        Effect::new(move |_| {
            let encoded = store.conn.get().encode();
            if let Some(storage) = storage() {
                let _ = storage.set_item(STORAGE_KEY, &encoded);
            }
        });
        store.persist_workspace();
        store
    }

    /// Sichert den Workspace bei Änderungen – gebündelt, nicht bei jedem Tastendruck.
    fn persist_workspace(&self) {
        let ws = self.ws;
        // Wartet ein Schreibvorgang schon? Dann reicht er für alle weiteren Änderungen.
        let pending = StoredValue::new(false);
        let write = move || {
            pending.set_value(false);
            if let Some(storage) = storage() {
                let text = ws.with_untracked(persist::save);
                let _ = storage.set_item(WORKSPACE_KEY, &text);
            }
        };
        // Der erste Lauf schreibt sofort: Ein alter oder kaputter Stand wird so überschrieben.
        Effect::new(move |first_run: Option<()>| {
            ws.track();
            if first_run.is_none() {
                write();
            } else if !pending.get_value() {
                pending.set_value(true);
                set_timeout(
                    move || {
                        if pending.try_get_value() == Some(true) {
                            write();
                        }
                    },
                    SAVE_DELAY,
                );
            }
        });
        // Wer die Seite verlässt oder neu lädt, verliert nichts von der letzten Sekunde.
        let flush = window_event_listener(leptos::ev::pagehide, move |_| {
            if pending.try_get_value() == Some(true) {
                write();
            }
        });
        on_cleanup(move || flush.remove());
    }

    /// Setzt die Beispieldaten zurück. Die Verbindungen bleiben.
    pub fn reset_demo(&self) {
        let ws = demo::workspace();
        self.selected.set(first_id(&ws));
        self.ws.set(ws);
    }

    /// Wählt einen Eintrag und zeigt ihn in der ungefilterten Inbox.
    pub fn open_item(&self, id: &str) {
        self.selected.set(id.to_string());
        navigate(&Route::Inbox(Filter::All));
    }
}

fn first_id(ws: &Workspace) -> String {
    ws.items.first().map(|i| i.id.clone()).unwrap_or_default()
}

pub fn use_store() -> Store {
    expect_context::<Store>()
}

pub fn current_hash() -> String {
    window().location().hash().unwrap_or_default()
}

/// Wechselt die Seite über die Adresse. Die App reagiert auf `hashchange`.
pub fn navigate(route: &Route) {
    let _ = window().location().set_hash(&route.hash());
}
