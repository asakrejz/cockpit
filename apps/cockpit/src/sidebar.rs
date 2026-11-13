//! The sidebar that all pages except setup share.

use cockpit_core::{Filter, team};
use feature_ui::*;
use leptos::prelude::*;

use crate::{route::Route, store::use_store};

/// Navigation of the app. The children form the middle, page-dependent section.
#[component]
pub fn AppSidebar(children: Children) -> impl IntoView {
    let store = use_store();
    let on = move |route: Route| Signal::derive(move || store.route.get() == route);
    let count = move |filter: Filter| {
        Signal::derive(move || {
            let now = store.now.get();
            store.ws.with(|w| w.unread(filter, now)).to_string()
        })
    };
    // Empty as long as nothing is snoozed.
    let snoozed = Signal::derive(move || {
        let now = store.now.get();
        match store.ws.with(|w| w.unread(Filter::Snoozed, now)) {
            0 => String::new(),
            n => n.to_string(),
        }
    });
    let stuck = Signal::derive(move || match store.ws.with(|w| team::stuck(w).len()) {
        0 => String::new(),
        n => format!("{n} hängt"),
    });
    let reviews = Signal::derive(move || store.ws.with(|w| w.open_reviews()).to_string());
    let agents = Signal::derive(move || {
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
    });

    // The entries exist twice: in the sidebar (wide screens) and in the bottom bar (up to 1100 px).
    let main_items = move || {
        view! {
            <SidebarItem
                href=Route::Inbox(Filter::All).hash()
                label="Aufmerksamkeit"
                icon=icons::BELL
                count=count(Filter::All)
                current=on(Route::Inbox(Filter::All))
            />
            <SidebarItem
                href=Route::Inbox(Filter::Mine).hash()
                label="Meine Aufgaben"
                icon=icons::TASKS
                count=count(Filter::Mine)
                current=on(Route::Inbox(Filter::Mine))
            />
            <SidebarItem
                href=Route::Inbox(Filter::Team).hash()
                label="Team"
                icon=icons::USERS
                count=count(Filter::Team)
                current=on(Route::Inbox(Filter::Team))
            />
            <SidebarItem
                href=Route::Inbox(Filter::Snoozed).hash()
                label="Zurückgestellt"
                icon=icons::MOON
                count=snoozed
                current=on(Route::Inbox(Filter::Snoozed))
            />
            <SidebarItem
                href=Route::Overview.hash()
                label="Übersicht"
                icon=icons::CHART
                count=stuck
                current=on(Route::Overview)
            />
            <SidebarItem
                href=Route::Review(None).hash()
                label="Pull Requests"
                icon=icons::GIT_PR
                count=reviews
                current=Signal::derive(move || matches!(store.route.get(), Route::Review(_)))
            />
            <SidebarItem
                href=Route::Agent(None).hash()
                label="Agenten"
                icon=icons::SPARKLES
                count=agents
                current=Signal::derive(move || matches!(store.route.get(), Route::Agent(_)))
            />
            <SidebarItem
                href=Route::Log.hash()
                label="Protokoll"
                icon=icons::LIBRARY
                current=on(Route::Log)
            />
        }
    };
    let more_items = move || {
        view! {
            <SidebarItem
                href=Route::Limits.hash()
                label="Grenzen & Regeln"
                icon=icons::SETTINGS
                current=on(Route::Limits)
            />
            <SidebarItem
                href=Route::Notifications.hash()
                label="Benachrichtigungen"
                icon=icons::BELL
                current=on(Route::Notifications)
            />
            <SidebarItem href=Route::Setup.hash() label="Einrichten" icon=icons::SLIDERS />
        }
    };

    // After every navigation start at the top (the browser keeps the old scroll position on
    // hash changes) and keep the current page in the sideways-scrolling bar in view.
    Effect::new(move |_| {
        store.route.track();
        window().scroll_to_with_x_and_y(0.0, 0.0);
        request_animation_frame(|| {
            if let Ok(Some(item)) =
                document().query_selector(".ui-bottomnav [aria-current=\"page\"]")
            {
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                options.set_inline(web_sys::ScrollLogicalPosition::Center);
                item.scroll_into_view_with_scroll_into_view_options(&options);
            }
        });
    });

    view! {
        <Sidebar label="Bereiche">
            <SidebarBrand href="#/">
                "Cockpit"
                <span title="Läuft lokal auf diesem Rechner">
                    <Badge variant=BadgeVariant::Neutral>"Lokal"</Badge>
                </span>
            </SidebarBrand>
            <SidebarSection role=SectionRole::Primary>{main_items()}</SidebarSection>
            {children()}
            <Spacer />
            <SidebarSection role=SectionRole::Primary>{more_items()}</SidebarSection>
        </Sidebar>
        <BottomNav label="Hauptnavigation">{main_items()}{more_items()}</BottomNav>
    }
}
