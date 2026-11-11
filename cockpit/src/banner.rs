//! Hinweise zum Server-Modus: wird verbunden, ist nicht erreichbar, oder der Server hat
//! eine Aktion abgelehnt. Im Demo-Modus erscheint nichts.

use feature_ui::*;
use leptos::prelude::*;

use crate::store::{Link, Mode, use_store};

#[component]
pub fn ServerBanner() -> impl IntoView {
    let store = use_store();
    let server = move || store.mode.get() == Mode::Server;
    let status = move || match (server(), store.link.get()) {
        (true, Link::Connecting) => Some("Verbinde mit dem Server …"),
        (true, Link::Offline) => Some("Der Server ist nicht erreichbar."),
        _ => None,
    };
    view! {
        <div class="ck-banner-stack">
            <Show when=move || status().is_some()>
                <div class="ck-banner" role="status">
                    <span class="t-subheadline">{status}</span>
                    <Show when=move || store.link.get() == Link::Offline>
                        <HStack spacing="8px">
                            <Button size=ControlSize::Small on_click=move |_| store.connect()>
                                "Erneut versuchen"
                            </Button>
                            <Button
                                size=ControlSize::Small
                                variant=ButtonStyle::GlassProminent
                                on_click=move |_| store.switch_to_demo()
                            >
                                "Zurück zu Demo"
                            </Button>
                        </HStack>
                    </Show>
                </div>
            </Show>
            <Show when=move || store.notice.get().is_some()>
                <div class="ck-banner" data-kind="notice" role="alert">
                    <span class="t-subheadline">{move || store.notice.get()}</span>
                    <Button
                        size=ControlSize::Small
                        variant=ButtonStyle::Borderless
                        on_click=move |_| store.notice.set(None)
                    >
                        "OK"
                    </Button>
                </div>
            </Show>
        </div>
    }
}
