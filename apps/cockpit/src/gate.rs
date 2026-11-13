//! What the app shows before the first state of the server has arrived: placeholder rows
//! while connecting, a plain error with a retry button when the server cannot be reached.
//! Afterwards the pages take over and a banner (`banner.rs`) reports later interruptions.

use cockpit_core::notice::{
    BROWSER_OFFLINE_TEXT, BROWSER_OFFLINE_TITLE, SERVER_UNREACHABLE_TEXT, SERVER_UNREACHABLE_TITLE,
};
use feature_ui::*;
use leptos::prelude::*;

use crate::store::{Link, use_store};

#[component]
pub fn Gate() -> impl IntoView {
    let store = use_store();
    view! {
        <main class="ck-gate">
            {move || {
                if store.link.get() == Link::Offline {
                    let (title, text) = if store.browser_online.get() {
                        (SERVER_UNREACHABLE_TITLE, SERVER_UNREACHABLE_TEXT)
                    } else {
                        (BROWSER_OFFLINE_TITLE, BROWSER_OFFLINE_TEXT)
                    };
                    view! {
                        <ErrorState title=title text=text>
                            <Button
                                size=ControlSize::Small
                                variant=ButtonStyle::GlassProminent
                                on_click=move |_| store.connect()
                            >
                                "Jetzt erneut versuchen"
                            </Button>
                        </ErrorState>
                    }
                        .into_any()
                } else {
                    view! {
                        <h1 class="t-title1 t-bold">"Cockpit"</h1>
                        <p class="t-subheadline t-secondary">"Verbinde mit dem Server …"</p>
                        <Skeleton rows=5 label="Cockpit lädt" />
                    }
                        .into_any()
                }
            }}
        </main>
    }
}
