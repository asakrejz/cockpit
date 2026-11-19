//! Cockpit: Jira, GitHub and agents in one place.
//!
//! The interface follows the design "Baukasten" and consists of the building blocks in
//! `feature-ui`. What the buttons do is in `cockpit-core`. This app connects both.

mod banner;
mod first_run;
mod gate;
mod language;
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
use feature_ui::{Root, Theme, lang};
use leptos::prelude::*;
use pages::{
    agent::AgentPage, inbox::InboxPage, limits::LimitsPage, log::LogPage,
    notifications::NotificationsPage, overview::OverviewPage, review::ReviewPage, setup::SetupPage,
};
use route::Route;
use store::{Store, current_hash, now_ms};

/// This often the app reads the clock again (snoozes, "vor 5 Min.").
const CLOCK_TICK: Duration = Duration::from_secs(15);

fn main() {
    // Rust panics end up readable in the browser console.
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// Light or dark, depending on the system.
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
    provide_context(sidebar::PageMenu::new());
    // The language follows the workspace (browser language until one is chosen).
    language::follow_workspace();
    // Return from the OAuth sign-in: show success or error.
    oauth_result::announce(store);
    // On the first start, when nothing is set up yet: to the assistant.
    first_run::check(store);

    // The address determines the page.
    let listener = window_event_listener(leptos::ev::hashchange, move |_| {
        store.route.set(Route::parse(&current_hash()));
    });
    on_cleanup(move || listener.remove());

    // The clock of attention: the pages calculate with `store.now`, not with a clock of their own.
    // Snoozing, linking, status sync and the tick of the agents run in the server.
    set_interval(move || store.now.set(now_ms()), CLOCK_TICK);

    // Memos, so that the page is only rebuilt when it really changes. A plain signal notifies at
    // every `set`, even with the same value: with `store.loaded` (set after every refresh of the
    // state) that rebuilt the whole page – and with it the setup assistant, back to step 1.
    let loaded = Memo::new(move |_| store.loaded.get());
    let page = Memo::new(move |_| store.route.with(Route::page));

    // Changing the language rebuilds the visible tree once, so every text is read again. The state
    // (store, workspace, route) lives above and stays.
    let content = move || {
        lang();
        view! {
            <ServerBanner />
                {move || {
                    if !loaded.get() {
                        return view! { <gate::Gate /> }.into_any();
                    }
                    match page.get() {
                    Route::Setup | Route::SetupStep(_) => view! { <SetupPage /> }.into_any(),
                    Route::Inbox(filter) => view! { <InboxPage filter=filter /> }.into_any(),
                    Route::Overview => view! { <OverviewPage /> }.into_any(),
                    Route::Log => view! { <LogPage /> }.into_any(),
                    Route::Notifications => view! { <NotificationsPage /> }.into_any(),
                    Route::Limits => view! { <LimitsPage /> }.into_any(),
                    Route::Review(number) => view! { <ReviewPage number=number /> }.into_any(),
                    Route::Agent(ticket) => view! { <AgentPage ticket=ticket /> }.into_any(),
                    // `page()` never returns the detail levels; they belong to the pages above.
                    Route::InboxItem(filter, _) => view! { <InboxPage filter=filter /> }.into_any(),
                    Route::OverviewMember(_) => view! { <OverviewPage /> }.into_any(),
                    }
                }}
                <Show when=move || store.loaded.get()>
                    <sidebar::AppNav />
                </Show>
                <toasts::Toasts />
        }
    };

    view! {
        <Root theme=system_theme() class="ck-root">{content}
        </Root>
    }
}
