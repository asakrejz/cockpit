//! The navigation of the app: a floating glass pill at the bottom edge on every screen width.
//!
//! The navigation targets are defined once ([`TARGETS`]): those with [`Slot::Bar`] sit in the
//! pill, the rest behind "More". The pill lives in the app shell (`main.rs`) so it stays when the
//! page changes (and its capsule can slide).

use cockpit_core::Filter;
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

/// One page of the navigation. "Aufgaben" stands for the whole inbox page: its filters
/// (Alle, Ich, Team, Zurückgestellt) are switched on the page itself.
struct Target {
    route: fn() -> Route,
    label: Key,
    /// Short label for the pill (a slot is only about 60 px wide at 320 px).
    short: Key,
    icon: &'static str,
    slot: Slot,
}

/// All targets, in the order of the pill and the sheet.
const TARGETS: [Target; 8] = [
    Target {
        route: || Route::Inbox(Filter::Mine),
        label: Key::NavTasks,
        short: Key::NavTasks,
        icon: icons::TASKS,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Review(None),
        label: Key::NavReviews,
        short: Key::NavReviewsShort,
        icon: icons::GIT_PR,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Agent(None),
        label: Key::NavAgents,
        short: Key::NavAgentsShort,
        icon: icons::SPARKLES,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Overview,
        label: Key::NavOverview,
        short: Key::NavOverviewShort,
        icon: icons::CHART,
        slot: Slot::Bar,
    },
    Target {
        route: || Route::Log,
        label: Key::NavLog,
        short: Key::NavLogShort,
        icon: icons::LIBRARY,
        slot: Slot::More,
    },
    Target {
        route: || Route::Limits,
        label: Key::NavLimits,
        short: Key::NavLimitsShort,
        icon: icons::SETTINGS,
        slot: Slot::More,
    },
    Target {
        route: || Route::Notifications,
        label: Key::NavNotifications,
        short: Key::NavNotificationsShort,
        icon: icons::BELL,
        slot: Slot::More,
    },
    Target {
        route: || Route::Setup,
        label: Key::NavSetup,
        short: Key::NavSetupShort,
        icon: icons::SLIDERS,
        slot: Slot::More,
    },
];

impl Target {
    /// Where the entry leads. "Aufgaben" leads to the filter chosen last.
    fn href(&self) -> String {
        match (self.route)() {
            Route::Inbox(_) => Route::TASKS_HASH.to_string(),
            other => other.hash(),
        }
    }

    fn label_text(&self) -> String {
        tr(self.label, &[])
    }

    fn short_text(&self) -> String {
        tr(self.short, &[])
    }

    /// Pull requests and agents stay current for every item they show.
    fn is_current(&self, route: &Route) -> bool {
        let own = (self.route)();
        match (&own, route) {
            (Route::Review(_), Route::Review(_)) | (Route::Agent(_), Route::Agent(_)) => true,
            (Route::Setup, other) => other.is_setup(),
            (Route::Inbox(_), Route::Inbox(_) | Route::InboxItem(..)) => true,
            _ => own == route.page(),
        }
    }

    /// Short text on the right of the entry in the sheet (empty: nothing).
    fn count_text(&self, store: Store) -> String {
        let now = store.now.get();
        match (self.route)() {
            Route::Inbox(_) => store.ws.with(|w| w.tasks_unread(now)).to_string(),
            Route::Review(_) => store.ws.with(|w| w.open_reviews()).to_string(),
            Route::Agent(_) => agent_summary(store),
            _ => String::new(),
        }
    }

    /// How many new things wait here (badge in the pill).
    fn attention(&self, store: Store) -> usize {
        let now = store.now.get();
        match (self.route)() {
            // Snoozed things are not new; "Ich" and the attention list count each entry once.
            Route::Inbox(_) => store.ws.with(|w| w.tasks_unread(now)),
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
        parts.push(t!(NavAgentsRunning, n = running));
    }
    if waiting > 0 {
        parts.push(t!(NavAgentsWaiting, n = waiting));
    }
    if paused > 0 {
        parts.push(t!(NavAgentsPaused, n = paused));
    }
    if stopped > 0 {
        parts.push(t!(NavAgentsStopped, n = stopped));
    }
    parts.join(" · ")
}

/// One state of a connection (Jira, GitHub, agent) as an entry that leads to the setup.
#[component]
pub fn ConnectionItem(
    #[prop(into)] label: String,
    on: fn(&cockpit_core::Connections) -> bool,
    /// What the green state says ("Connected", "Ready").
    text: Key,
) -> impl IntoView {
    let store = use_store();
    let connected = Signal::derive(move || on(&store.conn.get()));
    let status = view! {
        <Status tone=Signal::derive(move || {
            if connected.get() { StatusTone::Green } else { StatusTone::Gray }
        })>{move || if connected.get() { tr(text, &[]) } else { t!(NavDisconnected) }}</Status>
    };
    view! { <SidebarItem href=Route::Setup.hash() label=label trailing=status.into_any() /> }
}

/// The three connections.
#[component]
pub fn Connections() -> impl IntoView {
    view! {
        <ConnectionItem label="Jira" on=|c| c.jira text=Key::NavConnected />
        <ConnectionItem label="GitHub" on=|c| c.github text=Key::NavConnected />
        <ConnectionItem label=t!(NavAgent) on=|c| c.agent text=Key::NavReady />
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
                href=t.href()
                label=t.label_text()
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
                    href=t.href()
                    label=t.short_text()
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
            label=t!(NavMain)
            more_label=t!(NavMore)
            more_title=t!(NavMore)
            more_current=more_current
            more_badge=more_badge
            slots=bar_len + 1
            active=active
            more=move || {
                view! {
                    <SidebarBrand href=crate::route::Route::TASKS_HASH>
                        "Cockpit"
                        <span title=t!(NavLocalHint)>
                            <Badge variant=BadgeVariant::Neutral>{t!(NavLocal)}</Badge>
                        </span>
                    </SidebarBrand>
                    <ul class="ui-sidebar__nav">{more_items()}</ul>
                    {move || menu.0.get().map(|section| section.run())}
                    <p class="ui-sidebar__label">{t!(NavConnections)}</p>
                    <ul class="ui-sidebar__nav">
                        <Connections />
                    </ul>
                    <p class="ui-sidebar__label">{t!(LanguageLabel)}</p>
                    <div class="ck-sheet-lang">
                        <crate::language::LanguageSwitch stretch=true />
                    </div>
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
