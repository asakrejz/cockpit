//! Log: what was changed when and by whom, newest entries first.
//! The entries are written by `Workspace::apply` in `cockpit-core`; here they are only displayed.

use cockpit_core::{Actor, LogEntry, Target};
use feature_ui::*;
use leptos::prelude::*;
use web_sys::wasm_bindgen::JsValue;

use crate::{
    route::Route,
    sidebar::AppSidebar,
    store::{Store, use_store},
};

/// Filter by author. "all" shows everything.
const ACTORS: &[(&str, &str)] = &[
    ("all", "Alle"),
    ("du", "Du"),
    ("agent", "Agent"),
    ("system", "System"),
];

/// Time in the user's language, e.g. "11.11.2026, 14:03:22".
fn when(at_ms: u64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(at_ms as f64));
    date.to_locale_string("de-DE", &JsValue::UNDEFINED).into()
}

fn badge(actor: Actor) -> BadgeVariant {
    match actor {
        Actor::User => BadgeVariant::Neutral,
        Actor::Agent => BadgeVariant::Tint,
        Actor::System => BadgeVariant::Neutral,
    }
}

/// The way to the affected entry, review or run – if it still exists.
fn link(store: Store, target: Option<Target>) -> AnyView {
    let Some(target) = target else {
        return ().into_any();
    };
    match target {
        Target::Item(id) => {
            if !store.ws.with_untracked(|w| w.item(&id).is_some()) {
                return ().into_any();
            }
            view! {
                <Button size=ControlSize::Small on_click=move |_| store.open_item(&id)>
                    "Zum Eintrag"
                </Button>
            }
            .into_any()
        }
        Target::Review(number) => {
            if !store.ws.with_untracked(|w| w.review(&number).is_some()) {
                return ().into_any();
            }
            view! {
                <Button size=ControlSize::Small href=Route::Review(Some(number)).hash()>
                    "Review öffnen"
                </Button>
            }
            .into_any()
        }
        Target::Run(key) => {
            if !store.ws.with_untracked(|w| w.run(&key).is_some()) {
                return ().into_any();
            }
            view! {
                <Button size=ControlSize::Small href=Route::Agent(Some(key)).hash()>
                    "Lauf ansehen"
                </Button>
            }
            .into_any()
        }
        Target::Settings => view! {
            <Button size=ControlSize::Small href=Route::Notifications.hash()>
                "Einstellungen öffnen"
            </Button>
        }
        .into_any(),
    }
}

fn entry_row(store: Store, e: LogEntry) -> impl IntoView {
    view! {
        <li class="ck-stuck">
            <div class="ui-vstack" style="--spacing: 6px; min-width: 0; flex: 1 1 auto">
                <div class="ui-hstack" style="--spacing: 6px">
                    <Badge variant=badge(e.actor)>{e.actor.label()}</Badge>
                    <span class="t-footnote t-secondary">{when(e.at_ms)}</span>
                </div>
                <p class="t-headline">{e.text}</p>
                <div class="ui-hstack" style="--spacing: 8px">{link(store, e.target)}</div>
            </div>
        </li>
    }
}

fn empty_state(filtered: bool, actor: RwSignal<&'static str>) -> impl IntoView {
    let text = if filtered {
        "Von diesem Urheber gibt es noch keine Einträge."
    } else {
        "Sobald du etwas änderst oder der Agent arbeitet, steht es hier."
    };
    view! {
        <EmptyState title="Noch nichts protokolliert" text=text icon=icons::LIBRARY>
            {filtered
                .then(|| {
                    view! {
                        <Button size=ControlSize::Small on_click=move |_| actor.set("all")>
                            "Alle Urheber zeigen"
                        </Button>
                    }
                })}
        </EmptyState>
    }
}

#[component]
pub fn LogPage() -> impl IntoView {
    let store = use_store();
    // The filter belongs only to this page.
    let actor = RwSignal::new("all");
    let entries = Memo::new(move |_| {
        let who = Actor::from_id(actor.get());
        store
            .ws
            .with(|w| w.log_entries(who).into_iter().cloned().collect::<Vec<_>>())
    });

    view! {
        <SplitView>
            <AppSidebar>{""}</AppSidebar>

            <Pane label="Protokoll">
                <PaneHeader>
                    <div class="ui-hstack" style="justify-content: space-between; flex-wrap: nowrap">
                        <h1 class="t-large-title t-bold">"Protokoll"</h1>
                        <Badge variant=BadgeVariant::Count>{move || entries.get().len().to_string()}</Badge>
                    </div>
                    <p class="t-subheadline t-secondary">
                        "Jede Änderung mit Zeit und Urheber, neueste zuerst."
                    </p>
                    <SegmentedPicker
                        options=ACTORS
                        value=actor
                        on_change=move |id: &'static str| actor.set(id)
                        aria_label="Urheber"
                    />
                </PaneHeader>
                <PaneBody>
                    {move || {
                        let list = entries.get();
                        if list.is_empty() {
                            empty_state(actor.get() != "all", actor).into_any()
                        } else {
                            view! {
                                <ul class="ck-stuck-list">
                                    {list.into_iter().map(|e| entry_row(store, e)).collect_view()}
                                </ul>
                            }
                            .into_any()
                        }
                    }}
                </PaneBody>
            </Pane>
        </SplitView>
    }
}
