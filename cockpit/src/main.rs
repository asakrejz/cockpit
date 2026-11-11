//! Cockpit: Jira, GitHub und Agenten an einem Ort.
//!
//! Die Oberfläche folgt dem Design „Baukasten“ und besteht aus den Bausteinen in
//! `feature-ui`. Was die Knöpfe tun, steht in `cockpit-core`. Diese App verbindet beides.

mod banner;
mod mode_switch;
mod pages;
mod route;
mod server;
mod sidebar;
mod store;
mod view;

use std::time::Duration;

use banner::ServerBanner;
use cockpit_core::{Action, Actor};
use feature_ui::{Root, Theme};
use leptos::prelude::*;
use pages::{
    agent::AgentPage, inbox::InboxPage, log::LogPage, overview::OverviewPage, review::ReviewPage,
    setup::SetupPage,
};
use route::Route;
use store::{Mode, Store, current_hash, navigate};

/// So lange braucht der Demo-Agent für einen Schritt.
const AGENT_STEP: Duration = Duration::from_millis(2200);

fn main() {
    // Rust-Panics landen lesbar in der Browser-Konsole.
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// Hell oder dunkel, je nach System.
fn system_theme() -> Theme {
    let dark = window()
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
        .is_some_and(|query| query.matches());
    if dark { Theme::Dark } else { Theme::Light }
}

#[component]
fn App() -> impl IntoView {
    let store = Store::new();
    provide_context(store);

    // Die Adresse bestimmt die Seite.
    let listener = window_event_listener(leptos::ev::hashchange, move |_| {
        store.route.set(Route::parse(&current_hash()));
    });
    on_cleanup(move || listener.remove());

    // Ohne Verbindungen führt jeder Weg zum Einrichten.
    Effect::new(move |_| {
        if !store.conn.get().all_connected() && store.route.get() != Route::Setup {
            navigate(&Route::Setup);
        }
    });

    // Ein Takt für alle Agenten – sie arbeiten weiter, egal welche Seite offen ist.
    // Im Server-Modus macht den Takt der Server.
    set_interval(
        move || {
            if store.mode.get_untracked() == Mode::Demo
                && store.ws.with_untracked(|w| w.running_agents()) > 0
            {
                let _ = store.dispatch_as(Action::AdvanceAgents, Actor::Agent);
            }
        },
        AGENT_STEP,
    );

    view! {
        <Root theme=system_theme() class="ck-root">
            <ServerBanner />
            {move || match store.route.get() {
                Route::Setup => view! { <SetupPage /> }.into_any(),
                Route::Inbox(filter) => view! { <InboxPage filter=filter /> }.into_any(),
                Route::Overview => view! { <OverviewPage /> }.into_any(),
                Route::Log => view! { <LogPage /> }.into_any(),
                Route::Review(number) => view! { <ReviewPage number=number /> }.into_any(),
                Route::Agent(ticket) => view! { <AgentPage ticket=ticket /> }.into_any(),
            }}
        </Root>
    }
}
