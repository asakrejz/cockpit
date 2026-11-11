//! Gemeinsamer Zustand der App. Liegt als Kontext im Baum; `Store` ist `Copy`,
//! lässt sich also frei in Closures verwenden.

use std::time::Duration;

use cockpit_core::{Action, ActionError, Actor, Applied, Connections, Filter, Workspace, persist};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::route::Route;
use crate::server::{self, CallError, Events};

/// Unter diesem Namen liegen die Verbindungs-Haken im Browser. Schlüssel nie.
const STORAGE_KEY: &str = "cockpit.connections";
/// Hier liegt der Workspace (JSON mit Versionsnummer, siehe `cockpit_core::persist`).
/// Auch hier gilt: nur Daten des Cockpits, nie Schlüssel.
const WORKSPACE_KEY: &str = "cockpit.workspace";
/// Hier merkt sich der Browser den Modus: `demo` oder `server`.
const MODE_KEY: &str = "cockpit.mode";
/// So lange bleibt ein Hinweis nach einem Fehler sichtbar.
const NOTICE_TIME: Duration = Duration::from_secs(6);
/// Änderungen innerhalb dieser Zeit werden zu einem Schreibvorgang zusammengefasst.
const SAVE_DELAY: Duration = Duration::from_millis(400);

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

/// Wer den Workspace hält.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Der Browser wendet Aktionen selbst an, der Stand liegt in `localStorage`.
    Demo,
    /// Das lokale Backend hält den Stand; der Browser schickt Aktionen und hört zu.
    Server,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Demo => "demo",
            Mode::Server => "server",
        }
    }

    fn from_id(id: &str) -> Mode {
        if id == "server" {
            Mode::Server
        } else {
            Mode::Demo
        }
    }

    /// Der gemerkte Modus; ohne Eintrag oder bei Unsinn: Demo.
    pub fn saved() -> Mode {
        storage()
            .and_then(|s| s.get_item(MODE_KEY).ok().flatten())
            .map_or(Mode::Demo, |id| Mode::from_id(&id))
    }

    /// Merkt den Modus und lädt die Seite neu – der Modus gilt von Anfang an für die ganze App.
    pub fn switch_to(self) {
        if let Some(storage) = storage() {
            let _ = storage.set_item(MODE_KEY, self.id());
        }
        let _ = window().location().reload();
    }
}

/// Wie es um die Verbindung zum Server steht (nur im Server-Modus von Bedeutung).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub mode: Mode,
    pub link: RwSignal<Link>,
    /// Kurzer Hinweis, z. B. wenn der Server eine Aktion ablehnt. Verschwindet von selbst.
    pub notice: RwSignal<Option<String>>,
    /// Zuletzt übernommene Revision des Servers und ob schon ein Stand geladen ist.
    revision: StoredValue<Option<u64>>,
    loading: StoredValue<bool>,
    reload: StoredValue<bool>,
    events: StoredValue<Option<Events>, LocalStorage>,
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
        let mode = Mode::saved();
        let (ws, _) = match mode {
            Mode::Demo => {
                let saved_ws = storage().and_then(|s| s.get_item(WORKSPACE_KEY).ok().flatten());
                persist::restore(saved_ws.as_deref())
            }
            // Der Stand kommt vom Server. Der Demo-Stand im Browser bleibt unberührt.
            Mode::Server => (Workspace::default(), false),
        };
        let store = Store {
            mode,
            link: RwSignal::new(Link::Connecting),
            notice: RwSignal::new(None),
            revision: StoredValue::new(None),
            loading: StoredValue::new(false),
            reload: StoredValue::new(false),
            events: StoredValue::new_local(None),
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
        match mode {
            Mode::Demo => store.persist_workspace(),
            Mode::Server => store.connect(),
        }
        store
    }

    // ---------- Server-Modus ----------

    /// Zeigt kurz einen Hinweis (Fehler des Servers).
    pub fn notify(&self, text: impl Into<String>) {
        let text = text.into();
        self.notice.set(Some(text.clone()));
        let notice = self.notice;
        set_timeout(
            move || {
                // Nur wegräumen, was noch dasselbe ist – ein neuerer Hinweis bleibt.
                if notice.try_get_untracked().flatten().as_deref() == Some(text.as_str()) {
                    notice.set(None);
                }
            },
            NOTICE_TIME,
        );
    }

    /// Verbindet mit dem Server: lädt den Stand und hört auf Änderungen. Auch für „Erneut versuchen“.
    pub fn connect(&self) {
        let store = *self;
        store.link.set(Link::Connecting);
        store.events.set_value(None); // schließt eine alte Verbindung
        let events = Events::open(
            move || {
                // Auch nach einem Abbruch: Was inzwischen passiert ist, steht im neuen Stand.
                store.refresh();
            },
            move |event| {
                // Die Ereignisse tragen die neuen Protokolleinträge; den Stand holen wir
                // immer ganz. Stimmt die Revision, ist nichts zu tun. Eine Lücke braucht
                // denselben Weg wie eine einzelne Änderung.
                if store.revision.get_value() != Some(event.revision) {
                    store.refresh();
                }
            },
            move || store.link.set(Link::Offline),
        );
        store.events.set_value(events);
        store.refresh();
    }

    /// Lädt den Stand vom Server. Läuft schon ein Laden, wird danach noch einmal geladen.
    fn refresh(&self) {
        let store = *self;
        if store.loading.get_value() {
            store.reload.set_value(true);
            return;
        }
        store.loading.set_value(true);
        spawn_local(async move {
            loop {
                store.reload.set_value(false);
                match server::fetch_workspace().await {
                    Ok((ws, revision)) => {
                        store.link.set(Link::Online);
                        if store.revision.get_value() != Some(revision) {
                            store.revision.set_value(Some(revision));
                            store.adopt(ws);
                        }
                    }
                    Err(CallError::Unreachable) => store.link.set(Link::Offline),
                    Err(e) => {
                        store.link.set(Link::Offline);
                        store.notify(e.message());
                    }
                }
                if !store.reload.get_value() {
                    break;
                }
            }
            store.loading.set_value(false);
        });
    }

    /// Übernimmt den Stand des Servers; die Auswahl bleibt, wenn es den Eintrag noch gibt.
    fn adopt(&self, ws: Workspace) {
        let keep = self.selected.with_untracked(|id| ws.item(id).is_some());
        if !keep {
            self.selected.set(first_id(&ws));
        }
        self.ws.set(ws);
    }

    /// Schickt die Aktion an den Server. Angezeigt wird erst der Stand, den der Server meldet.
    fn send(&self, action: Action) {
        let store = *self;
        spawn_local(async move {
            match server::post_action(action).await {
                Ok(response) => {
                    store.link.set(Link::Online);
                    if store.revision.get_value() != Some(response.revision) {
                        store.refresh();
                    }
                }
                Err(CallError::Unreachable) => {
                    store.link.set(Link::Offline);
                    store.notify("Der Server ist nicht erreichbar – die Änderung ging nicht durch");
                }
                Err(e) => store.notify(e.message()),
            }
        });
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
        if self.mode == Mode::Server {
            // Der Server wendet an (als „Du“, mit seiner Uhr) und meldet das Ergebnis über den
            // Ereignisstrom. Fehler erscheinen als Hinweis. Den Takt der Agenten macht er selbst.
            if actor == Actor::Du {
                self.send(action);
            }
            return Ok(Applied {
                summary: String::new(),
                logged: 0,
            });
        }
        let reset = action == Action::ResetDemo;
        // Die Zeit kommt aus dem Browser; die Logik in `cockpit-core` kennt keine Uhr.
        let at_ms = js_sys::Date::now() as u64;
        // Erst anwenden, dann melden: Bei einem Fehler soll sich nichts neu zeichnen.
        let mut work = self.ws.get_untracked();
        let result = work.apply(action, actor, at_ms);
        if result.is_ok() {
            if reset {
                self.selected.set(first_id(&work));
            }
            self.ws.set(work);
        }
        result
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
