//! The sidebar that all pages except setup share, and the bottom navigation of narrow screens.
//!
//! The navigation targets are defined once ([`TARGETS`]): wide screens list all of them in the
//! sidebar, narrow screens show those with [`Slot::Bar`] in the fixed bar and the rest behind
//! "Mehr".

use cockpit_core::{Filter, team};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    route::Route,
    store::{Store, use_store},
};

/// Where a target sits on narrow screens (up to 1100 px).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Fixed in the bottom bar.
    Bar,
    /// In the sheet behind "Mehr".
    More,
}

/// One page of the navigation.
struct Target {
    route: fn() -> Route,
    label: &'static str,
    /// Short label for the bottom bar (a slot is only about 64 px wide at 320 px).
    short: &'static str,
    icon: &'static str,
    slot: Slot,
    /// Wide screens: in the lower group of the sidebar instead of the upper one.
    footer: bool,
}

/// All targets, in the order of the sidebar (and of the sheet).
const TARGETS: [Target; 11] = [
    Target {
        route: || Route::Inbox(Filter::All),
        label: "Aufmerksamkeit",
        short: "Inbox",
        icon: icons::BELL,
        slot: Slot::Bar,
        footer: false,
    },
    Target {
        route: || Route::Inbox(Filter::Mine),
        label: "Meine Aufgaben",
        short: "Aufgaben",
        icon: icons::TASKS,
        slot: Slot::Bar,
        footer: false,
    },
    Target {
        route: || Route::Inbox(Filter::Team),
        label: "Team",
        short: "Team",
        icon: icons::USERS,
        slot: Slot::More,
        footer: false,
    },
    Target {
        route: || Route::Inbox(Filter::Snoozed),
        label: "Zurückgestellt",
        short: "Zurückgestellt",
        icon: icons::MOON,
        slot: Slot::More,
        footer: false,
    },
    Target {
        route: || Route::Overview,
        label: "Übersicht",
        short: "Übersicht",
        icon: icons::CHART,
        slot: Slot::More,
        footer: false,
    },
    Target {
        route: || Route::Review(None),
        label: "Pull Requests",
        short: "Reviews",
        icon: icons::GIT_PR,
        slot: Slot::Bar,
        footer: false,
    },
    Target {
        route: || Route::Agent(None),
        label: "Agenten",
        short: "Agenten",
        icon: icons::SPARKLES,
        slot: Slot::Bar,
        footer: false,
    },
    Target {
        route: || Route::Log,
        label: "Protokoll",
        short: "Protokoll",
        icon: icons::LIBRARY,
        slot: Slot::More,
        footer: false,
    },
    Target {
        route: || Route::Limits,
        label: "Grenzen & Regeln",
        short: "Grenzen",
        icon: icons::SETTINGS,
        slot: Slot::More,
        footer: true,
    },
    Target {
        route: || Route::Notifications,
        label: "Benachrichtigungen",
        short: "Mitteilungen",
        icon: icons::BELL,
        slot: Slot::More,
        footer: true,
    },
    Target {
        route: || Route::Setup,
        label: "Einrichten",
        short: "Einrichten",
        icon: icons::SLIDERS,
        slot: Slot::More,
        footer: true,
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

    /// Short text on the right of the sidebar entry (empty: nothing).
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

    /// How many new things wait here (badge in the bottom bar).
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

/// Navigation of the app. The children form the middle, page-dependent section.
#[component]
pub fn AppSidebar(children: Children) -> impl IntoView {
    let store = use_store();
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
    let sidebar_group = move |footer: bool| {
        TARGETS
            .iter()
            .filter(|t| t.footer == footer)
            .map(entry)
            .collect_view()
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
    let more_current = Signal::derive(move || {
        let route = store.route.get();
        TARGETS
            .iter()
            .any(|t| t.slot == Slot::More && t.is_current(&route))
    });
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
        <Sidebar label="Bereiche">
            <SidebarBrand href="#/">
                "Cockpit"
                <span title="Läuft lokal auf diesem Rechner">
                    <Badge variant=BadgeVariant::Neutral>"Lokal"</Badge>
                </span>
            </SidebarBrand>
            <SidebarSection role=SectionRole::Primary>{sidebar_group(false)}</SidebarSection>
            {children()}
            <Spacer />
            <SidebarSection role=SectionRole::Primary>{sidebar_group(true)}</SidebarSection>
        </Sidebar>
        <BottomNav
            label="Hauptnavigation"
            more_label="Mehr"
            more_title="Mehr"
            more_current=more_current
            more_badge=more_badge
            more=move || {
                view! {
                    <ul class="ui-sidebar__nav">{more_items()}</ul>
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
