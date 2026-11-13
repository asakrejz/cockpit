//! State of the sync with Jira and GitHub per source, plus "Jetzt abgleichen".
//! The data is in the workspace (`sync`), the sync runs in the
//! backend. No key is here.

use cockpit_core::{Source, SourceState};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server;
use crate::store::{Store, now_ms, use_store};

fn subtitle(state: &SourceState, now: u64) -> String {
    let last = state
        .last_ok_ms
        .map_or("noch nie abgeglichen".to_string(), |t| {
            format!("zuletzt abgeglichen {}", cockpit_core::notice::ago(t, now))
        });
    match &state.problem {
        None => match state.warnings.first() {
            Some(w) => format!("{last} · Hinweis: {w}"),
            None => last,
        },
        Some(p) => format!("{} · {last}", p.text()),
    }
}

/// "Jetzt abgleichen": asks the server to sync now and reports the outcome as a notice.
pub fn sync_now(store: Store, busy: RwSignal<bool>) {
    busy.set(true);
    spawn_local(async move {
        let result = server::post_sync().await;
        busy.set(false);
        match result {
            Ok(r) if !r.active => store
                .notify("Keine Quelle verbunden – richte Jira oder GitHub unter „Einrichten“ ein"),
            Ok(_) => {} // The state comes through the event stream.
            Err(e) => store.notify(e.message()),
        }
    });
}

#[component]
pub fn SyncStatus() -> impl IntoView {
    let store = use_store();
    let busy = RwSignal::new(false);

    let sync_now = move |_| sync_now(store, busy);

    let rows = move || {
        let now = now_ms();
        let states = store.ws.with(|ws| ws.sync.sources.clone());
        if states.is_empty() {
            return view! {
                <ListRow
                    title="Keine Quelle abgeglichen"
                    subtitle="Jira und GitHub richtest du unter „Einrichten“ ein. Bis dahin bleibt der Workspace leer."
                    wrap=true
                />
            }
            .into_any();
        }
        states
            .into_iter()
            .map(|st| {
                let name = match st.source {
                    Source::Jira => "Jira",
                    Source::GitHub => "GitHub",
                };
                let ok = st.is_ok();
                view! {
                    <ListRow title=name subtitle=subtitle(&st, now) wrap=true>
                        <Badge variant=if ok { BadgeVariant::Tint } else { BadgeVariant::Neutral }>
                            {if ok { "verbunden" } else { "Problem" }}
                        </Badge>
                    </ListRow>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <List>
                <ListSection
                    header="Abgleich"
                    footer="Das Backend fragt Jira und GitHub regelmäßig ab (Standard: alle 2 Minuten). Aktionen wie Antworten, Status, Review oder Mergen gehen erst an den Dienst und werden danach hier angewendet."
                >
                    {rows}
                    <ListRow title="Jetzt abgleichen" subtitle="Holt Tickets, Pull Requests, Kommentare und Checks sofort">
                        <Button disabled=Signal::derive(move || busy.get()) on_click=sync_now>
                            {move || if busy.get() { "Gleiche ab …" } else { "Jetzt abgleichen" }}
                        </Button>
                    </ListRow>
                </ListSection>
        </List>
    }
}
