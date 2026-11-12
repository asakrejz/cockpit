//! Gemeinsamer Zustand der App. Liegt als Kontext im Baum; `Store` ist `Copy`,
//! lässt sich also frei in Closures verwenden. Der Stand gehört dem lokalen Server; der Browser
//! zeigt ihn, schickt Aktionen und hört auf Änderungen. Im Browser liegt nichts davon.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use cockpit_core::{
    Action, ActionError, Actor, Applied, Channel, Connections, Filter, NotifyEvent, Workspace,
    notify,
};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server::{self, CallError, Events};
use crate::{
    notify::{self as browser, Permission},
    route::Route,
};

/// So lange bleibt ein Hinweis nach einem Fehler sichtbar.
const NOTICE_TIME: Duration = Duration::from_secs(6);
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

/// Wie es um die Verbindung zum Server steht .
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub link: RwSignal<Link>,
    /// Kurzer Hinweis, z. B. wenn der Server eine Aktion ablehnt. Verschwindet von selbst.
    pub notice: RwSignal<Option<String>>,
    /// Zuletzt übernommene Revision des Servers und ob schon ein Stand geladen ist.
    revision: StoredValue<Option<u64>>,
    loading: StoredValue<bool>,
    reload: StoredValue<bool>,
    events: StoredValue<Option<Events>, LocalStorage>,
    pub ws: RwSignal<Workspace>,
    /// Was das Backend über die Anmeldung bei Jira und GitHub meldet (`GET /api/health`).
    pub conn: RwSignal<Connections>,
    /// Agent-Endpunkt und Container-Laufzeit, wie das Backend sie meldet; `None` bis zur ersten Antwort.
    pub health: RwSignal<Option<server::Health>>,
    /// ID des Eintrags, der in der Inbox gewählt ist.
    pub selected: RwSignal<String>,
    pub route: RwSignal<Route>,
    /// Hinweise, die gerade zu sehen sind.
    pub toasts: RwSignal<Vec<Toast>>,
    /// Erlaubnis für Systemmeldungen des Browsers.
    pub permission: RwSignal<Permission>,
    /// Die Uhrzeit (Millisekunden seit 1970), wie sie der Takt zuletzt gelesen hat. Die
    /// Regeln für Aufmerksamkeit und Zurückstellen rechnen damit, nicht mit einer eigenen Uhr.
    pub now: RwSignal<u64>,
}

impl Store {
    pub fn new() -> Self {
        // Der Stand kommt vom Server; bis dahin ist der Workspace leer.
        let ws = Workspace::default();
        let store = Store {
            link: RwSignal::new(Link::Connecting),
            notice: RwSignal::new(None),
            revision: StoredValue::new(None),
            loading: StoredValue::new(false),
            reload: StoredValue::new(false),
            events: StoredValue::new_local(None),
            selected: RwSignal::new(first_id(&ws)),
            ws: RwSignal::new(ws),
            // Ob der Agent bereit ist, Jira und GitHub verbunden sind, meldet das Backend.
            conn: RwSignal::new(Connections::default()),
            health: RwSignal::new(None),
            route: RwSignal::new(Route::parse(&current_hash())),
            toasts: RwSignal::new(Vec::new()),
            permission: RwSignal::new(browser::permission()),
            now: RwSignal::new(now_ms()),
        };
        store.connect();
        store
    }

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
            move || {
                store.link.set(Link::Offline);
            },
        );
        store.events.set_value(events);
        store.refresh();
        store.refresh_health();
    }

    /// Holt Anmeldung, Agent-Endpunkt und Container-Laufzeit vom Server.
    pub fn refresh_health(&self) {
        let store = *self;
        spawn_local(async move {
            if let Ok(health) = server::fetch_health().await {
                store.conn.update(|c| {
                    c.apply_services(&health.anmeldung.github, &health.anmeldung.jira);
                    c.agent = health.agent.bereit;
                });
                store.health.set(Some(health));
            }
        });
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
                let result = server::fetch_workspace().await;
                match result {
                    Ok((ws, revision)) => {
                        store.link.set(Link::Online);
                        let previous = store.revision.get_value();
                        if previous != Some(revision) {
                            store.revision.set_value(Some(revision));
                            // Beim ersten Laden ist alles „neu“ – das soll nicht klingeln.
                            store.adopt(ws, previous.is_some());
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
    /// Mit `announce` meldet sie, was sich gegenüber dem bisherigen Stand geändert hat.
    fn adopt(&self, ws: Workspace, announce: bool) {
        if announce {
            let before = self.ws.get_untracked();
            self.announce_changes(&before, &ws);
        }
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
            let result = server::post_action(action).await;
            match result {
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

    /// Der einzige Weg, den Workspace zu ändern: schickt die Aktion als „Du“ an den Server, der
    /// sie anwendet und protokolliert. Das Ergebnis erscheint über den Ereignisstrom; Fehler als
    /// Hinweis.
    pub fn dispatch(&self, action: Action) -> Result<Applied, ActionError> {
        self.dispatch_as(action, Actor::Du)
    }

    /// Wie [`Store::dispatch`], für Änderungen, die nicht vom Nutzer kommen. Die macht der Server
    /// selbst (Takt der Agenten, Abgleich); der Browser schickt nur, was „Du“ tust.
    pub fn dispatch_as(&self, action: Action, actor: Actor) -> Result<Applied, ActionError> {
        if actor == Actor::Du {
            self.send(action);
        }
        Ok(Applied {
            summary: String::new(),
            logged: 0,
        })
    }

    /// Leitet aus dem Vorher/Nachher die Ereignisse ab und meldet die für den Browser.
    /// Läuft nach dem Laden eines neuen Stands.
    fn announce_changes(&self, before: &Workspace, after: &Workspace) {
        let events = notify::events(before, after);
        let deliveries =
            notify::deliveries(&events, &after.notifications, browser::minute_of_day());
        for delivery in deliveries {
            // E-Mail und Slack versendet später das Backend.
            if delivery.channel == Channel::Browser {
                self.announce(delivery.event, false);
            }
        }
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

/// Die Uhr des Browsers. Nur hier wird sie gelesen; `cockpit-core` bekommt die Zeit übergeben.
pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// Abstand UTC − Ortszeit in Minuten, wie ihn `Date.getTimezoneOffset()` liefert.
pub fn utc_offset_min() -> i32 {
    js_sys::Date::new_0().get_timezone_offset() as i32
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
