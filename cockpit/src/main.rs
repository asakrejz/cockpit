//! Cockpit: Jira, GitHub und Agenten an einem Ort.
//!
//! Die Oberfläche folgt dem Design „Baukasten“ und besteht aus den Bausteinen in
//! `feature-ui`. Was die Knöpfe tun, steht in `cockpit-core`. Diese App verbindet beides.

mod banner;
mod notify;
mod oauth_card;
mod oauth_result;
mod pages;
mod route;
mod server;
mod sidebar;
mod store;
mod sync_status;
mod toasts;
mod view;

use std::time::Duration;

use banner::ServerBanner;
use feature_ui::{Root, Theme};
use leptos::prelude::*;
use pages::{
    agent::AgentPage, inbox::InboxPage, limits::LimitsPage, log::LogPage,
    notifications::NotificationsPage, overview::OverviewPage, review::ReviewPage, setup::SetupPage,
};
use route::Route;
use store::{Store, current_hash, now_ms};

/// So oft liest die App die Uhr neu (Zurückstellungen, „vor 5 Min.“).
const CLOCK_TICK: Duration = Duration::from_secs(15);

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
    // Rückkehr von der Anmeldung per OAuth: Erfolg oder Fehler anzeigen.
    oauth_result::announce(store);

    // Die Adresse bestimmt die Seite.
    let listener = window_event_listener(leptos::ev::hashchange, move |_| {
        store.route.set(Route::parse(&current_hash()));
    });
    on_cleanup(move || listener.remove());

    // Die Uhr der Aufmerksamkeit: Die Seiten rechnen mit `store.now`, nicht mit einer eigenen Uhr.
    // Zurückstellen, Verknüpfen, Statusabgleich und der Takt der Agenten laufen im Server.
    set_interval(move || store.now.set(now_ms()), CLOCK_TICK);

    view! {
        <Root theme=system_theme() class="ck-root">
            <ServerBanner />
            {move || match store.route.get() {
                Route::Setup => view! { <SetupPage /> }.into_any(),
                Route::Inbox(filter) => view! { <InboxPage filter=filter /> }.into_any(),
                Route::Overview => view! { <OverviewPage /> }.into_any(),
                Route::Log => view! { <LogPage /> }.into_any(),
                Route::Notifications => view! { <NotificationsPage /> }.into_any(),
                Route::Limits => view! { <LimitsPage /> }.into_any(),
                Route::Review(number) => view! { <ReviewPage number=number /> }.into_any(),
                Route::Agent(ticket) => view! { <AgentPage ticket=ticket /> }.into_any(),
            }}
            <toasts::Toasts />
        </Root>
    }
}
