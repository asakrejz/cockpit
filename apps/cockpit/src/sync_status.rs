//! Zustand des Abgleichs mit Jira und GitHub je Quelle, dazu „Jetzt abgleichen“.
//! Die Daten stehen im Workspace (`sync`), der Abgleich läuft im
//! Backend. Hier steht kein Schlüssel.

use cockpit_core::{Source, SourceState};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server::{self, CallError};
use crate::store::{now_ms, use_store};

/// „vor 5 Min.“ – grob genug für eine Statuszeile.
fn ago(then_ms: u64, now: u64) -> String {
    let s = now.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => "gerade eben".into(),
        60..=3599 => format!("vor {} Min.", s / 60),
        3600..=86_399 => format!("vor {} Std.", s / 3600),
        _ => format!("vor {} Tagen", s / 86_400),
    }
}

fn subtitle(state: &SourceState, now: u64) -> String {
    let last = state
        .last_ok_ms
        .map_or("noch nie abgeglichen".to_string(), |t| {
            format!("zuletzt abgeglichen {}", ago(t, now))
        });
    match &state.problem {
        None => match state.warnings.first() {
            Some(w) => format!("{last} · Hinweis: {w}"),
            None => last,
        },
        Some(p) => format!("{} · {last}", p.text()),
    }
}

#[component]
pub fn SyncStatus() -> impl IntoView {
    let store = use_store();
    let busy = RwSignal::new(false);

    let sync_now = move |_| {
        busy.set(true);
        spawn_local(async move {
            let result = server::post_sync().await;
            busy.set(false);
            match result {
                Ok(r) if !r.active => store.notify(
                    "Keine Quelle verbunden – richte Jira oder GitHub unter „Einrichten“ ein",
                ),
                Ok(_) => {} // Der Stand kommt über den Ereignisstrom.
                Err(CallError::Unreachable) => store.notify("Der Server ist nicht erreichbar"),
                Err(e) => store.notify(e.message()),
            }
        });
    };

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
