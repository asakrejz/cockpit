//! Die Sidebar, die alle Seiten außer dem Einrichten teilen.

use cockpit_core::{Filter, team};
use feature_ui::*;
use leptos::prelude::*;

use crate::{route::Route, store::use_store};

/// Navigation der App. Die Kinder bilden den mittleren, seitenabhängigen Abschnitt.
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
    // Leer, solange nichts zurückgestellt ist.
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
        let (running, waiting, paused) = store
            .ws
            .with(|w| (w.running_agents(), w.waiting_agents(), w.paused_agents()));
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
        parts.join(" · ")
    });

    view! {
        <Sidebar label="Bereiche">
            <SidebarBrand href="#/">
                "Cockpit"
                <span title="Daten und Verbindungen sind simuliert">
                    <Badge variant=BadgeVariant::Neutral>"Demo"</Badge>
                </span>
            </SidebarBrand>
            <SidebarSection>
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
            </SidebarSection>
            {children()}
            <Spacer />
            <SidebarSection>
                <SidebarItem
                    href=Route::Notifications.hash()
                    label="Benachrichtigungen"
                    icon=icons::BELL
                    current=on(Route::Notifications)
                />
                <SidebarItem href=Route::Setup.hash() label="Einstellungen" icon=icons::SLIDERS />
            </SidebarSection>
        </Sidebar>
    }
}
