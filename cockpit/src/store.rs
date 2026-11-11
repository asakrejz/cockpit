//! Gemeinsamer Zustand der App. Liegt als Kontext im Baum; `Store` ist `Copy`,
//! lässt sich also frei in Closures verwenden.

use cockpit_core::{Connections, Filter, Workspace, demo};
use leptos::prelude::*;

use crate::route::Route;

/// Unter diesem Namen liegen die Verbindungs-Haken im Browser. Schlüssel nie.
const STORAGE_KEY: &str = "cockpit.connections";

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
        let ws = demo::workspace();
        let store = Store {
            selected: RwSignal::new(ws.items.first().map(|i| i.id.clone()).unwrap_or_default()),
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
        store
    }

    /// Wählt einen Eintrag und zeigt ihn in der ungefilterten Inbox.
    pub fn open_item(&self, id: &str) {
        self.selected.set(id.to_string());
        navigate(&Route::Inbox(Filter::All));
    }
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
