//! Gemeinsamer Zustand der App. Liegt als Kontext im Baum; `Store` ist `Copy`,
//! lässt sich also frei in Closures verwenden.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use cockpit_core::{
    Action, ActionError, Actor, Applied, Channel, Connections, Filter, NotifyEvent, Workspace,
    notify, persist,
};
use leptos::prelude::*;

use crate::{
    notify::{self as browser, Permission},
    route::Route,
};

/// Unter diesem Namen liegen die Verbindungs-Haken im Browser. Schlüssel nie.
const STORAGE_KEY: &str = "cockpit.connections";
/// Hier liegt der Workspace (JSON mit Versionsnummer, siehe `cockpit_core::persist`).
/// Auch hier gilt: nur Daten des Cockpits, nie Schlüssel.
const WORKSPACE_KEY: &str = "cockpit.workspace";
/// Änderungen innerhalb dieser Zeit werden zu einem Schreibvorgang zusammengefasst.
const SAVE_DELAY: Duration = Duration::from_millis(400);
/// So lange bleibt ein Hinweis in der App stehen.
const TOAST_TIME: Duration = Duration::from_secs(7);
/// Mehr Hinweise gleichzeitig wären zu viel.
const MAX_TOASTS: usize = 3;

/// Ein kleiner Hinweis in der App (Toast).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub event: NotifyEvent,
}

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
    /// Hinweise, die gerade zu sehen sind.
    pub toasts: RwSignal<Vec<Toast>>,
    /// Erlaubnis für Systemmeldungen des Browsers.
    pub permission: RwSignal<Permission>,
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
            toasts: RwSignal::new(Vec::new()),
            permission: RwSignal::new(browser::permission()),
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

    /// Der einzige Weg, den Workspace zu ändern: wendet die Aktion als „Du“ an und
    /// protokolliert sie. Fehler kommen zurück und ändern nichts.
    pub fn dispatch(&self, action: Action) -> Result<Applied, ActionError> {
        self.dispatch_as(action, Actor::Du)
    }

    /// Wie [`Store::dispatch`], für Änderungen, die nicht vom Nutzer kommen (Takt des Agenten).
    pub fn dispatch_as(&self, action: Action, actor: Actor) -> Result<Applied, ActionError> {
        let reset = action == Action::ResetDemo;
        // Die Zeit kommt aus dem Browser; die Logik in `cockpit-core` kennt keine Uhr.
        let at_ms = js_sys::Date::now() as u64;
        // Erst anwenden, dann melden: Bei einem Fehler soll sich nichts neu zeichnen.
        let before = self.ws.get_untracked();
        let mut work = before.clone();
        let result = work.apply(action, actor, at_ms);
        if result.is_ok() {
            if reset {
                self.selected.set(first_id(&work));
            }
            // Beim Zurücksetzen kommt alles „neu“ – das soll nicht klingeln.
            let events = if reset {
                Vec::new()
            } else {
                notify::events(&before, &work)
            };
            let deliveries =
                notify::deliveries(&events, &work.notifications, browser::minute_of_day());
            self.ws.set(work);
            for delivery in deliveries {
                // E-Mail und Slack versendet später das Backend.
                if delivery.channel == Channel::Browser {
                    self.announce(delivery.event, false);
                }
            }
        }
        result
    }

    /// Meldet ein Ereignis: im Vordergrund als Hinweis in der App, sonst als Systemmeldung.
    /// Mit `both` (Test) kommt beides.
    pub fn announce(&self, event: NotifyEvent, both: bool) {
        let visible = browser::foreground();
        if visible || both {
            self.toast(event.clone());
        }
        if !visible || both {
            browser::show_system(&event);
        }
    }

    fn toast(&self, event: NotifyEvent) {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        self.toasts.update(|list| {
            list.push(Toast { id, event });
            if list.len() > MAX_TOASTS {
                list.remove(0);
            }
        });
        let store = *self;
        set_timeout(move || store.dismiss(id), TOAST_TIME);
    }

    pub fn dismiss(&self, id: u32) {
        let _ = self.toasts.try_update(|list| list.retain(|t| t.id != id));
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

/// Wechselt zu einer Adresse wie `#/review/87`.
pub fn navigate_hash(hash: &str) {
    let _ = window().location().set_hash(hash);
}

/// Wechselt die Seite über die Adresse. Die App reagiert auf `hashchange`.
pub fn navigate(route: &Route) {
    let _ = window().location().set_hash(&route.hash());
}
