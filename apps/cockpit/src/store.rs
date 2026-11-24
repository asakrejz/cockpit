//! Shared state of the app. Lies in the tree as context; `Store` is `Copy`,
//! so it can be used freely in closures. The state belongs to the local server; the browser
//! shows it, sends actions and listens for changes. Nothing of it is in the browser.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use cockpit_core::{Action, ActionError, Actor, Applied, Channel, NotifyEvent, Workspace, notify};
use cockpit_i18n::{Msg, msg};
use feature_ui::t;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server::{self, CallError, Events};
use crate::{
    notify::{self as browser, Permission},
    route::Route,
};

/// This long a notice stays visible after an error.
const NOTICE_TIME: Duration = Duration::from_secs(6);
/// This long a notice stays in the app.
const TOAST_TIME: Duration = Duration::from_secs(7);
/// More notices at the same time would be too much.
const MAX_TOASTS: usize = 3;

/// A small notice in the app (toast).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub event: NotifyEvent,
}

/// How the connection to the server stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub link: RwSignal<Link>,
    /// The first state of the server has arrived. Until then pages show a loading or error state
    /// instead of an empty workspace.
    pub loaded: RwSignal<bool>,
    /// The browser itself has no network (`navigator.onLine`).
    pub browser_online: RwSignal<bool>,
    /// Failed reconnect attempts in a row (drives the back-off).
    attempts: StoredValue<u32>,
    retry_pending: StoredValue<bool>,
    /// Short notice, e.g. when the server rejects an action. Disappears by itself.
    pub notice: RwSignal<Option<String>>,
    /// Last revision taken over from the server and whether a state is loaded already.
    revision: StoredValue<Option<u64>>,
    loading: StoredValue<bool>,
    reload: StoredValue<bool>,
    events: StoredValue<Option<Events>, LocalStorage>,
    pub ws: RwSignal<Workspace>,
    /// What the backend reports about sign-in, agent endpoint and container runtime
    /// (`GET /api/health`); `None` until the first answer.
    pub health: RwSignal<Option<server::Health>>,
    /// ID of the entry that is chosen in the inbox.
    pub selected: RwSignal<String>,
    pub route: RwSignal<Route>,
    /// Notices that are visible right now.
    pub toasts: RwSignal<Vec<Toast>>,
    /// Permission for system messages of the browser.
    pub permission: RwSignal<Permission>,
    /// The time of day (milliseconds since 1970) as the tick last read it. The
    /// rules for attention and snoozing calculate with it, not with a clock of their own.
    pub now: RwSignal<u64>,
}

impl Store {
    pub fn new() -> Self {
        // The state comes from the server; until then the workspace is empty.
        let ws = Workspace::default();
        let store = Store {
            link: RwSignal::new(Link::Connecting),
            loaded: RwSignal::new(false),
            browser_online: RwSignal::new(window().navigator().on_line()),
            attempts: StoredValue::new(0),
            retry_pending: StoredValue::new(false),
            notice: RwSignal::new(None),
            revision: StoredValue::new(None),
            loading: StoredValue::new(false),
            reload: StoredValue::new(false),
            events: StoredValue::new_local(None),
            selected: RwSignal::new(first_id(&ws)),
            ws: RwSignal::new(ws),
            health: RwSignal::new(None),
            route: RwSignal::new(Route::resolve(&current_hash(), crate::last_filter::get())),
            toasts: RwSignal::new(Vec::new()),
            permission: RwSignal::new(browser::permission()),
            now: RwSignal::new(now_ms()),
        };
        store.connect();
        store.watch_network();
        store
    }

    /// Follows the network of the browser; coming back online reconnects at once.
    fn watch_network(&self) {
        let store = *self;
        window_event_listener(leptos::ev::offline, move |_| {
            store.browser_online.set(false)
        });
        window_event_listener(leptos::ev::online, move |_| {
            store.browser_online.set(true);
            if store.link.get_untracked() != Link::Online {
                store.attempts.set_value(0);
                store.connect();
            }
        });
    }

    /// The server is not reachable: say so and try again by itself, with growing pauses.
    fn lost(&self) {
        let store = *self;
        store.link.set(Link::Offline);
        if store.retry_pending.get_value() {
            return; // A retry is already planned.
        }
        store.retry_pending.set_value(true);
        let attempt = store.attempts.get_value();
        store.attempts.set_value(attempt + 1);
        let wait = Duration::from_secs(u64::from(cockpit_core::notice::reconnect_delay_s(attempt)));
        set_timeout(
            move || {
                store.retry_pending.set_value(false);
                if store.link.try_get_untracked() == Some(Link::Offline) {
                    store.reconnect();
                }
            },
            wait,
        );
    }

    /// A quiet attempt: keeps the "offline" banner until it works (no flicker to "connecting").
    fn reconnect(&self) {
        let store = *self;
        store.events.set_value(None);
        let events = Events::open(
            move || store.refresh(),
            move |event| {
                if store.revision.get_value() != Some(event.revision) {
                    store.refresh();
                }
            },
            move || store.lost(),
        );
        store.events.set_value(events);
        store.refresh();
        store.refresh_health();
    }

    /// Briefly shows a notice (error of the server).
    pub fn notify(&self, text: impl Into<String>) {
        let text = text.into();
        self.notice.set(Some(text.clone()));
        let notice = self.notice;
        set_timeout(
            move || {
                // Only clear away what is still the same – a newer notice stays.
                if notice.try_get_untracked().flatten().as_deref() == Some(text.as_str()) {
                    notice.set(None);
                }
            },
            NOTICE_TIME,
        );
    }

    /// Connects to the server: loads the state and listens for changes. Also for "Erneut versuchen".
    pub fn connect(&self) {
        let store = *self;
        store.link.set(Link::Connecting);
        store.events.set_value(None); // closes an old connection
        let events = Events::open(
            move || {
                // Also after an interruption: what happened in the meantime is in the new state.
                store.refresh();
            },
            move |event| {
                // The events carry the new log entries; we always fetch the state
                // completely. If the revision matches, there is nothing to do. A gap needs
                // the same way as a single change.
                if store.revision.get_value() != Some(event.revision) {
                    store.refresh();
                }
            },
            move || store.lost(),
        );
        store.events.set_value(events);
        store.refresh();
        store.refresh_health();
    }

    /// Marks the server as reachable. Only writes on a change: a signal notifies at every `set`.
    fn set_online(&self) {
        if self.link.get_untracked() != Link::Online {
            self.link.set(Link::Online);
        }
    }

    /// Fetches sign-in, agent endpoint and container runtime from the server.
    pub fn refresh_health(&self) {
        let store = *self;
        spawn_local(async move {
            if let Ok(health) = server::fetch_health().await {
                store.health.set(Some(health));
            }
        });
    }

    /// Is GitHub connected? Follows the health report; `false` until the first answer.
    pub fn github_connected(&self) -> bool {
        self.reported(|h| h.sign_in.github.connected)
    }

    /// Is Jira connected? Follows the health report; `false` until the first answer.
    pub fn jira_connected(&self) -> bool {
        self.reported(|h| h.sign_in.jira.connected)
    }

    /// Can the agent start? Follows the health report; `false` until the first answer.
    pub fn agent_ready(&self) -> bool {
        self.reported(|h| h.agent.ready)
    }

    /// Why the agent may not implement a ticket – or `None` if it may.
    pub fn agent_blocked(&self) -> Option<Msg> {
        (!self.agent_ready()).then(|| msg!(CoreMiscAgentNotConnected))
    }

    fn reported(&self, read: impl Fn(&server::Health) -> bool) -> bool {
        self.health.with(|h| h.as_ref().is_some_and(read))
    }

    /// Loads the state from the server. If a load is already running, it loads once more afterwards.
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
                        store.set_online();
                        if !store.loaded.get_untracked() {
                            store.loaded.set(true);
                        }
                        store.attempts.set_value(0);
                        let previous = store.revision.get_value();
                        if previous != Some(revision) {
                            store.revision.set_value(Some(revision));
                            // On the first load everything is "new" – that should not ring.
                            store.adopt(ws, previous.is_some());
                        }
                    }
                    Err(CallError::Unreachable) => store.lost(),
                    Err(e) => {
                        store.lost();
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

    /// Takes over the state of the server; the selection stays if the entry still exists.
    /// With `announce` it reports what has changed compared to the previous state.
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

    /// Sends the action to the server. Only the state that the server reports is displayed.
    fn send(&self, action: Action) {
        let store = *self;
        spawn_local(async move {
            let result = server::post_action(action).await;
            match result {
                Ok(response) => {
                    store.set_online();
                    if store.revision.get_value() != Some(response.revision) {
                        store.refresh();
                    }
                }
                Err(CallError::Unreachable) => {
                    store.lost();
                    store.notify(t!(StoreUnreachableChange));
                }
                Err(e) => store.notify(e.message()),
            }
        });
    }

    /// The only way to change the workspace: sends the action as "Du" to the server, which
    /// applies and logs it. The result appears through the event stream; errors as a
    /// notice.
    pub fn dispatch(&self, action: Action) -> Result<Applied, ActionError> {
        self.dispatch_as(action, Actor::User)
    }

    /// Like [`Store::dispatch`], for changes that do not come from the user. The server does those
    /// itself (tick of the agents, sync); the browser only sends what "Du" does.
    pub fn dispatch_as(&self, action: Action, actor: Actor) -> Result<Applied, ActionError> {
        if actor == Actor::User {
            self.send(action);
        }
        Ok(Applied { logged: 0 })
    }

    /// Derives the events from the before/after and reports those for the browser.
    /// Runs after loading a new state.
    fn announce_changes(&self, before: &Workspace, after: &Workspace) {
        let events = notify::events(before, after);
        let deliveries =
            notify::deliveries(&events, &after.notifications, browser::minute_of_day());
        for delivery in deliveries {
            match delivery.channel {
                Channel::Browser => self.announce(delivery.event, false),
            }
        }
    }

    /// Reports an event: in the foreground as a notice in the app, otherwise as a system message.
    /// With `both` (test) both come.
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

    /// Chooses an entry and shows exactly that entry in the inbox: the address carries its ID
    /// (on a narrow screen that opens the detail level). If a filter hides the entry, the inbox
    /// shows it anyway and says so.
    pub fn open_item(&self, id: &str) {
        self.selected.set(id.to_string());
        navigate(&Route::to_entry(id));
    }
}

/// The clock of the browser. Only here is it read; `cockpit-core` gets the time passed in.
pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// Difference UTC − local time in minutes, as `Date.getTimezoneOffset()` returns it.
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

/// Switches to an address like `#/review/87`.
pub fn navigate_hash(hash: &str) {
    let _ = window().location().set_hash(hash);
}

/// Changes the address without a new entry in the history (the back button skips it), e.g. when an
/// address without details is completed. `hashchange` does not fire for this, so the route is set here.
impl Store {
    pub fn replace_route(&self, route: &Route) {
        let url = route.hash();
        let replaced = window().history().ok().is_some_and(|h| {
            h.replace_state_with_url(&web_sys::wasm_bindgen::JsValue::NULL, "", Some(&url))
                .is_ok()
        });
        if replaced {
            self.route.set(route.clone());
        } else {
            navigate(route);
        }
    }
}

impl Store {
    /// Takes the route from the address. A tasks address without a filter is replaced (no new
    /// history entry) by the one of the filter chosen last.
    pub fn apply_hash(&self) {
        let hash = current_hash();
        let route = Route::resolve(&hash, crate::last_filter::get());
        if Route::is_tasks_alias(&hash) {
            self.replace_route(&route);
        } else {
            self.route.set(route);
        }
    }
}

/// Switches the page via the address. The app reacts to `hashchange`.
pub fn navigate(route: &Route) {
    let _ = window().location().set_hash(&route.hash());
}
