//! What the app shows before the first state of the server has arrived: placeholder rows
//! while connecting, a plain error with a retry button when the server cannot be reached.
//! Afterwards the pages take over and a banner (`banner.rs`) reports later interruptions.

use cockpit_core::notice::{
    browser_offline_text, browser_offline_title, server_unreachable_text, server_unreachable_title,
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
                        (server_unreachable_title(), server_unreachable_text())
                    } else {
                        (browser_offline_title(), browser_offline_text())
                    };
                    let (title, text) = (title.render(lang()), text.render(lang()));
                    view! {
                        <ErrorState title=title text=text>
                            <Button
                                size=ControlSize::Small
                                variant=ButtonStyle::GlassProminent
                                on_click=move |_| store.connect()
                            >
                                {t!(BannerRetry)}
                            </Button>
                        </ErrorState>
                    }
                        .into_any()
                } else {
                    view! {
                        <h1 class="t-title1 t-bold">"Cockpit"</h1>
                        <p class="t-subheadline t-secondary">{t!(BannerConnecting)}</p>
                        <Skeleton rows=5 label=t!(GateLoading) />
                    }
                        .into_any()
                }
            }}
        </main>
    }
}
