//! Einstellung „Demo“ oder „Lokaler Server“. Der Wechsel lädt die App neu, damit der
//! Modus von Anfang an für alles gilt. Zum Server wechselt nur, wer ihn erreicht
//! (`GET /api/health`).

use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server;
use crate::store::{Mode, use_store};

const OPTIONS: &[(&str, &str)] = &[("demo", "Demo"), ("server", "Lokaler Server")];

/// Was die Suche nach dem Server ergeben hat.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Probe {
    Checking,
    Found,
    Missing,
}

#[component]
pub fn ModeSwitch() -> impl IntoView {
    let store = use_store();
    let probe = RwSignal::new(Probe::Checking);
    let check = move || {
        probe.set(Probe::Checking);
        spawn_local(async move {
            let found = server::health_ok().await;
            probe.set(if found { Probe::Found } else { Probe::Missing });
        });
    };
    check();

    let choose = Callback::new(move |id: &'static str| {
        if id == store.mode.id() {
            return;
        }
        if id == Mode::Demo.id() {
            Mode::Demo.switch_to();
            return;
        }
        // Erst prüfen, dann wechseln: Ohne Server bleibt die App im Demo-Modus.
        spawn_local(async move {
            if server::health_ok().await {
                Mode::Server.switch_to();
            } else {
                probe.set(Probe::Missing);
                store.notify("Kein Server gefunden – bleibt bei Demo");
            }
        });
    });

    let hint = move || match (store.mode, probe.get()) {
        (_, Probe::Checking) => "Suche den lokalen Server …",
        (Mode::Server, _) => {
            "Der Server hält den Stand und lässt den Agenten arbeiten. Demo-Daten im Browser bleiben unberührt."
        }
        (Mode::Demo, Probe::Found) => "Ein lokaler Server ist erreichbar.",
        (Mode::Demo, Probe::Missing) => {
            "Kein Server erreichbar. Starte ihn mit „cargo run -p cockpit-server“ und öffne die App unter http://127.0.0.1:8787."
        }
    };

    view! {
        <List>
            <ListSection header="Modus">
                <ListRow
                    title="Daten und Agent"
                    subtitle="Demo rechnet im Browser, der lokale Server hält den Stand"
                    wrap=true
                >
                    <SegmentedPicker
                        options=OPTIONS
                        value=Signal::derive(move || store.mode.id())
                        on_change=choose
                        aria_label="Modus"
                    />
                </ListRow>
            </ListSection>
        </List>
        <p class="t-footnote t-secondary" style="padding-inline: 4px" role="status">{hint}</p>
    }
}
