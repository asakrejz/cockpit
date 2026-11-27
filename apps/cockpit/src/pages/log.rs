//! Log: what was changed when and by whom, newest entries first.
//! Here the entries are only displayed.

use cockpit_core::{Actor, LogEntry, Target};
use feature_ui::*;
use leptos::prelude::*;
use web_sys::wasm_bindgen::JsValue;

use crate::{
    pages::inbox::picker::Picker,
    route::Route,
    store::{Store, use_store},
};

/// Filter by author. "all" shows everything.
fn actors() -> Vec<(&'static str, String)> {
    vec![
        ("all", t!(LogActorAll)),
        (Actor::User.id(), actor_label(Actor::User)),
        (Actor::Agent.id(), actor_label(Actor::Agent)),
        (Actor::System.id(), actor_label(Actor::System)),
    ]
}

fn actor_label(actor: Actor) -> String {
    match actor {
        Actor::User => t!(LogActorUser),
        Actor::Agent => t!(LogActorAgent),
        Actor::System => t!(LogActorSystem),
    }
}

/// Time in the user's language, e.g. "11.11.2026, 14:03:22".
fn when(at_ms: u64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(at_ms as f64));
    date.to_locale_string(lang().code(), &JsValue::UNDEFINED)
        .into()
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
                    {t!(LogToEntry)}
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
                    {t!(LogOpenReview)}
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
                    {t!(LogViewRun)}
                </Button>
            }
            .into_any()
        }
        Target::Settings => view! {
            <Button size=ControlSize::Small href=Route::Notifications.hash()>
                {t!(LogOpenSettings)}
            </Button>
        }
        .into_any(),
    }
}

fn entry_row(store: Store, e: LogEntry) -> impl IntoView {
    view! {
        <li class="ck-stuck">
            <div class="ui-vstack ui-gap-6 ui-grow">
                <div class="ui-hstack ui-gap-6">
                    <Badge variant=badge(e.actor)>{actor_label(e.actor)}</Badge>
                    <span class="t-footnote t-secondary">{when(e.at_ms)}</span>
                </div>
                <p class="t-headline">{e.text_in(lang())}</p>
                <div class="ui-hstack ui-gap-8">{link(store, e.target)}</div>
            </div>
        </li>
    }
}

fn empty_state(filtered: bool, actor: RwSignal<&'static str>) -> impl IntoView {
    let text = if filtered {
        t!(LogEmptyFiltered)
    } else {
        t!(LogEmpty)
    };
    view! {
        <EmptyState title=t!(LogEmptyTitle) text=text icon=icons::LIBRARY>
            {filtered
                .then(|| {
                    view! {
                        <Button size=ControlSize::Small on_click=move |_| actor.set("all")>
                            {t!(LogShowAll)}
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

            <Pane label=t!(LogTitle)>
                <PaneHeader>
                    <div class="ui-hstack ui-justify-between ui-flex-nowrap">
                        <h1 class="t-large-title t-bold">{t!(LogTitle)}</h1>
                        <Badge variant=BadgeVariant::Count>{move || entries.get().len().to_string()}</Badge>
                    </div>
                    <p class="t-subheadline t-secondary">
                        {t!(LogSubtitle)}
                    </p>
                    <Picker
                        options=actors()
                        value=actor
                        on_change=move |id: &'static str| actor.set(id)
                        aria_label=t!(LogAuthor)
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
