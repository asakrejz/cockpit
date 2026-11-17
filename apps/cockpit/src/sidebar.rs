//! The navigation of the app: a floating glass pill at the bottom edge on every screen width.
//!
//! The navigation targets are defined once ([`TARGETS`]): those with [`Slot::Bar`] sit in the
//! pill, the rest behind "Mehr". The pill lives in the app shell (`main.rs`) so it stays when the
//! page changes (and its capsule can slide).

use cockpit_core::{Filter, team};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    route::Route,
    store::{Store, use_store},
};

/// Where a target sits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// In the pill.
    Bar,
    /// In the sheet behind "Mehr".
    More,
}

/// One page of the navigation.
struct Target {
    route: fn() -> Route,
    label: &'static str,
    /// Short label for the pill (a slot is only about 60 px wide at 320 px).
    short: &'static str,
    icon: &'static str,
    slot: Slot,
}

/// All targets, in the order of the pill and the sheet.
const TARGETS: [Target; 11] = [
    Target {
        route: || Route::Inbox(Filter::All),
        label: "Aufmerksamkeit",
        short: "Inbox",
        icon: icons::BELL,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Inbox(Filter::Mine),
        label: "Meine Aufgaben",
        short: "Aufgaben",
        icon: icons::TASKS,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Inbox(Filter::Team),
        label: "Team",
        short: "Team",
        icon: icons::USERS,
        slot: Slot::More,
    },
    Target {
        route: || Route::Inbox(Filter::Snoozed),
        label: "Zurückgestellt",
        short: "Zurückgestellt",
        icon: icons::MOON,
        slot: Slot::More,
    },
    Target {
        route: || Route::Overview,
        label: "Übersicht",
        short: "Übersicht",
        icon: icons::CHART,
        slot: Slot::More,
    },
    Target {
        route: || Route::Review(None),
        label: "Pull Requests",
        short: "Reviews",
        icon: icons::GIT_PR,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Agent(None),
        label: "Agenten",
        short: "Agenten",
        icon: icons::SPARKLES,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Log,
        label: "Protokoll",
        short: "Protokoll",
        icon: icons::LIBRARY,
        slot: Slot::More,
    },
    Target {
        route: || Route::Limits,
        label: "Grenzen & Regeln",
        short: "Grenzen",
        icon: icons::SETTINGS,
        slot: Slot::More,
    },
    Target {
        route: || Route::Notifications,
        label: "Benachrichtigungen",
        short: "Mitteilungen",
        icon: icons::BELL,
        slot: Slot::More,
    },
    Target {
        route: || Route::Setup,
        label: "Einrichten",
        short: "Einrichten",
        icon: icons::SLIDERS,
        slot: Slot::More,
    },
];

impl Target {
    /// Pull requests and agents stay current for every item they show.
    fn is_current(&self, route: &Route) -> bool {
        let own = (self.route)();
        match (&own, route) {
            (Route::Review(_), Route::Review(_)) | (Route::Agent(_), Route::Agent(_)) => true,
            (Route::Setup, other) => other.is_setup(),
            _ => &own == route,
        }
    }

    /// Short text on the right of the entry in the sheet (empty: nothing).
    fn count_text(&self, store: Store) -> String {
        let now = store.now.get();
        match (self.route)() {
            Route::Inbox(Filter::Snoozed) => {
                match store.ws.with(|w| w.unread(Filter::Snoozed, now)) {
                    0 => String::new(),
                    n => n.to_string(),
                }
            }
            Route::Inbox(filter) => store.ws.with(|w| w.unread(filter, now)).to_string(),
            Route::Overview => match store.ws.with(|w| team::stuck(w).len()) {
                0 => String::new(),
                n => format!("{n} hängt"),
            },
            Route::Review(_) => store.ws.with(|w| w.open_reviews()).to_string(),
            Route::Agent(_) => agent_summary(store),
            _ => String::new(),
        }
    }

    /// How many new things wait here (badge in the pill).
    fn attention(&self, store: Store) -> usize {
        let now = store.now.get();
        match (self.route)() {
            // Snoozed things are not new.
            Route::Inbox(Filter::Snoozed) => 0,
            Route::Inbox(filter) => store.ws.with(|w| w.unread(filter, now)),
            Route::Overview => store.ws.with(|w| team::stuck(w).len()),
            Route::Review(_) => store.ws.with(|w| w.open_reviews()),
            Route::Agent(_) => store.ws.with(|w| w.waiting_agents()),
            _ => 0,
        }
    }
}

fn agent_summary(store: Store) -> String {
    let (running, waiting, paused, stopped) = store.ws.with(|w| {
        (
            w.running_agents(),
            w.waiting_agents(),
            w.paused_agents(),
            w.stopped_agents(),
        )
    });
    // Halted ones already count as "waiting"; here is only how many of them hit a limit.
    let waiting = waiting - stopped;
    let mut parts = Vec::new();
    if running > 0 {
        parts.push(format!("{running} aktiv"));
    }
    if waiting > 0 {
        parts.push(format!("{waiting} wartet"));
    }
    if paused > 0 {
        parts.push(format!("{paused} pausiert"));
    }
    if stopped > 0 {
        parts.push(format!("{stopped} angehalten"));
    }
    parts.join(" · ")
}

/// One state of a connection (Jira, GitHub, agent) as an entry that leads to the setup.
#[component]
pub fn ConnectionItem(
    label: &'static str,
    on: fn(&cockpit_core::Connections) -> bool,
    /// What the green state says ("Verbunden", "Bereit").
    text: &'static str,
) -> impl IntoView {
    let store = use_store();
    let connected = Signal::derive(move || on(&store.conn.get()));
    let status = view! {
        <Status tone=Signal::derive(move || {
            if connected.get() { StatusTone::Green } else { StatusTone::Gray }
        })>{move || if connected.get() { text } else { "Getrennt" }}</Status>
    };
    view! { <SidebarItem href=Route::Setup.hash() label=label trailing=status.into_any() /> }
}

/// The three connections.
#[component]
pub fn Connections() -> impl IntoView {
    view! {
        <ConnectionItem label="Jira" on=|c| c.jira text="Verbunden" />
        <ConnectionItem label="GitHub" on=|c| c.github text="Verbunden" />
        <ConnectionItem label="Agent" on=|c| c.agent text="Bereit" />
    }
}

/// Sections that the current page adds to the "Mehr" sheet (under the targets, above the
/// connections). Provided by the app shell.
#[derive(Clone, Copy)]
pub struct PageMenu(RwSignal<Option<ViewFn>>);

impl PageMenu {
    pub fn new() -> Self {
        Self(RwSignal::new(None))
    }
}

/// Which slot of the pill the route belongs to: the index in the bar, "Mehr" (after the bar
/// targets) for the targets in the sheet, `None` for a page that is no target.
fn active_slot(route: &Route) -> Option<usize> {
    let bar = TARGETS.iter().filter(|t| t.slot == Slot::Bar);
    let bar_len = bar.clone().count();
    for (index, t) in bar.enumerate() {
        if t.is_current(route) {
            return Some(index);
        }
    }
    TARGETS
        .iter()
        .any(|t| t.slot == Slot::More && t.is_current(route))
        .then_some(bar_len)
}

/// The navigation pill with the "Mehr" sheet.
#[component]
pub fn AppNav() -> impl IntoView {
    let store = use_store();
    let menu = expect_context::<PageMenu>();
    let entry = move |t: &'static Target| {
        let current = Signal::derive(move || t.is_current(&store.route.get()));
        let count = Signal::derive(move || t.count_text(store));
        view! {
            <SidebarItem
                href=(t.route)().hash()
                label=t.label
                icon=t.icon
                count=count
                current=current
            />
        }
    };
    let bar_items = TARGETS
        .iter()
        .filter(|t| t.slot == Slot::Bar)
        .map(|t| {
            view! {
                <BottomNavItem
                    href=(t.route)().hash()
                    label=t.short
                    icon=t.icon
                    current=Signal::derive(move || t.is_current(&store.route.get()))
                    badge=Signal::derive(move || t.attention(store))
                />
            }
        })
        .collect_view();
    let more_items = move || {
        TARGETS
            .iter()
            .filter(|t| t.slot == Slot::More)
            .map(entry)
            .collect_view()
    };
    let active = Signal::derive(move || active_slot(&store.route.get()));
    let bar_len = TARGETS.iter().filter(|t| t.slot == Slot::Bar).count();
    let more_current = Signal::derive(move || active.get() == Some(bar_len));
    let more_badge = Signal::derive(move || {
        TARGETS
            .iter()
            .filter(|t| t.slot == Slot::More)
            .map(|t| t.attention(store))
            .sum::<usize>()
    });

    // After every navigation start at the top (the browser keeps the old scroll position on
    // hash changes).
    Effect::new(move |_| {
        store.route.track();
        window().scroll_to_with_x_and_y(0.0, 0.0);
    });

    view! {
        <BottomNav
            label="Hauptnavigation"
            more_label="Mehr"
            more_title="Mehr"
            more_current=more_current
            more_badge=more_badge
            slots=bar_len + 1
            active=active
            more=move || {
                view! {
                    <SidebarBrand href="#/">
                        "Cockpit"
                        <span title="Läuft lokal auf diesem Rechner">
                            <Badge variant=BadgeVariant::Neutral>"Lokal"</Badge>
                        </span>
                    </SidebarBrand>
                    <ul class="ui-sidebar__nav">{more_items()}</ul>
                    {move || menu.0.get().map(|section| section.run())}
                    <p class="ui-sidebar__label">"Verbindungen"</p>
                    <ul class="ui-sidebar__nav">
                        <Connections />
                    </ul>
                }
                .into_any()
            }
        >
            {bar_items}
        </BottomNav>
    }
}

#[cfg(test)]
mod tests;
