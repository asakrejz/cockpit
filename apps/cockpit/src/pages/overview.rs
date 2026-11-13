//! Overview: on the left who works on what and how full they are; on the right what is stuck, and
//! the entries of the chosen person. The rules for it are in `cockpit_core::team`.

use cockpit_core::{Item, Member, Person, Stuck, StuckKind, Tone, team};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    route::Route,
    sidebar::AppSidebar,
    store::{Store, use_store},
    view,
};

/// Line "1 Offen · 2 In Arbeit" – only what exists.
fn counts(m: &Member) -> String {
    let parts: Vec<String> = [
        (m.open, "Offen"),
        (m.in_progress, "In Arbeit"),
        (m.review, "Review"),
        (m.pull_requests, "Pull Request"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, label)| match (n, label) {
        (1, _) => format!("{n} {label}"),
        (_, "Pull Request") => format!("{n} Pull Requests"),
        _ => format!("{n} {label}"),
    })
    .collect();
    if parts.is_empty() {
        "Nichts offen".into()
    } else {
        parts.join(" · ")
    }
}

fn kind_icon(kind: StuckKind) -> &'static str {
    match kind {
        StuckKind::ChecksFailed => icons::ALERT,
        StuckKind::AgentStopped => icons::ALERT,
        StuckKind::QuestionWaiting => icons::SPARKLES,
        StuckKind::DiagnosisWaiting => icons::SPARKLES,
        StuckKind::PlanWaiting => icons::TASKS,
        StuckKind::AgentWaiting => icons::SPARKLE,
        StuckKind::ReviewWaiting => icons::EYE,
        StuckKind::NoBranch => icons::GIT_BRANCH,
    }
}

fn kind_tone(kind: StuckKind) -> Tone {
    match kind {
        StuckKind::ChecksFailed => Tone::Red,
        StuckKind::AgentStopped => Tone::Red,
        StuckKind::QuestionWaiting => Tone::Orange,
        StuckKind::DiagnosisWaiting => Tone::Orange,
        StuckKind::PlanWaiting => Tone::Orange,
        StuckKind::AgentWaiting => Tone::Green,
        StuckKind::ReviewWaiting => Tone::Orange,
        StuckKind::NoBranch => Tone::Indigo,
    }
}

fn person_avatar(person: &Person) -> impl IntoView + use<> {
    view! {
        <Avatar
            initials=person.initials.clone()
            agent=person.is_agent()
            title=person.name.clone()
        />
    }
}

#[component]
pub fn OverviewPage() -> impl IntoView {
    let store = use_store();
    let members = Memo::new(move |_| store.ws.with(team::members));
    let stuck = Memo::new(move |_| store.ws.with(team::stuck));

    // Chosen person, by name. If they disappear (nothing open any more), "Du" applies.
    let chosen = RwSignal::new(Person::me().name);
    let current = Memo::new(move |_| {
        let name = chosen.get();
        let list = members.get();
        list.iter()
            .find(|m| m.person.name == name)
            .or(list.first())
            .cloned()
    });

    view! {
           <SplitView>
               <AppSidebar>{""}</AppSidebar>

               <Pane label="Team">
                   <PaneHeader>
                       <h1 class="t-large-title t-bold">"Übersicht"</h1>
                       <p class="t-subheadline t-secondary">
                           "Wer woran arbeitet und wie viel gerade offen ist."
                       </p>
                   </PaneHeader>
                   <ul class="ck-inbox">
                       {move || {
                           let active = current.get().map(|m| m.person.name).unwrap_or_default();
                           members
                               .get()
                               .into_iter()
                               .map(|m| {
    let selected = m.person.name == active;
    member_row(m, selected, chosen)
    })
                               .collect_view()
                       }}
                   </ul>
               </Pane>

               <Pane label="Details">
                   <PaneHeader>
                       <div class="ui-hstack" style="justify-content: space-between; flex-wrap: nowrap">
                           <h2 class="t-title1 t-bold">"Hängt"</h2>
                           <Badge variant=BadgeVariant::Count>{move || stuck.get().len().to_string()}</Badge>
                       </div>
                   </PaneHeader>
                   <PaneBody>
                       {move || {
                           let list = stuck.get();
                           if list.is_empty() {
                               empty_state().into_any()
                           } else {
                               view! {
                                   <ul class="ck-stuck-list">
                                       {list.into_iter().map(|s| stuck_row(store, s)).collect_view()}
                                   </ul>
                               }
                                   .into_any()
                           }
                       }}
                       {move || current.get().map(|m| person_items(store, m))}
                   </PaneBody>
               </Pane>
           </SplitView>
       }
}

/// A person in the left list: numbers by status and the workload.
fn member_row(m: Member, selected: bool, chosen: RwSignal<String>) -> impl IntoView {
    let name = m.person.name.clone();
    let total = m.total();
    let level = m.load_level();
    let summary = format!(
        "{}: {} von {} Plätzen belegt",
        m.person.name,
        total.min(team::CAPACITY),
        team::CAPACITY
    );
    view! {
        <InboxRow selected=selected on_select=move |_| chosen.set(name.clone())>
            {person_avatar(&m.person)}
            <span class="ui-vstack" style="--spacing: 6px; flex: 1 1 auto; min-width: 0">
                <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                    <span class="t-headline ck-ellipsis" style="flex: 1 1 auto">{m.person.name.clone()}</span>
                    <Badge variant=BadgeVariant::Neutral>{total.to_string()}</Badge>
                </span>
                <span class="t-subheadline t-secondary">{counts(&m)}</span>
                <Progress value=total.min(team::CAPACITY) max=team::CAPACITY label=summary />
                <span class="t-footnote t-secondary">{level.label()}</span>
            </span>
        </InboxRow>
    }
}

/// No entry is stuck: that is good news and should look like it.
fn empty_state() -> impl IntoView {
    view! {
        <div class="ck-empty-state">
            <span class="ck-empty-state__icon" aria-hidden="true">
                <Icon path=icons::CHECK size=22 stroke=2.4 />
            </span>
            <p class="t-headline">"Nichts hängt."</p>
            <p class="t-subheadline t-secondary">
                "Keine fehlgeschlagenen Checks, keine wartenden Reviews und kein Ticket ohne Branch."
            </p>
        </div>
    }
}

/// An entry that is stuck: reason, who is on it, and the way there.
fn stuck_row(store: Store, s: Stuck) -> impl IntoView {
    let id = s.item_id.clone();
    let key = s.key.clone();
    let tone = view::tone(kind_tone(s.kind)).id();
    view! {
        <li class="ck-stuck">
            <span
                class="ui-list__icon ck-reason"
                style=format!("background: var(--sys-color-{tone})")
                aria-hidden="true"
            >
                <Icon path=kind_icon(s.kind) size=13 stroke=2.4 />
            </span>
            <div class="ui-vstack" style="--spacing: 6px; min-width: 0">
                <div class="ui-hstack" style="--spacing: 6px">
                    <Badge variant=BadgeVariant::Neutral>{s.kind.label()}</Badge>
                    <span class="t-footnote t-mono t-secondary">{s.key}</span>
                </div>
                <p class="t-headline">{s.title}</p>
                <p class="t-subheadline t-secondary">{s.reason}</p>
                <div class="ui-hstack" style="--spacing: 8px">
                    <Button size=ControlSize::Small on_click=move |_| store.open_item(&id)>
                        "Zum Eintrag"
                    </Button>
                    {s
                        .plan
                        .clone()
                        .map(|key| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key)).hash()>
                                    "Plan ansehen"
                                </Button>
                            }
                        })}
                    {s
                        .question
                        .clone()
                        .map(|key| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key)).hash()>
                                    "Frage beantworten"
                                </Button>
                            }
                        })}
                    {(s.kind == StuckKind::AgentStopped)
                        .then(|| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key.clone())).hash()>
                                    "Limit erhöhen oder abbrechen"
                                </Button>
                            }
                        })}
                    {s
                        .diagnosis
                        .clone()
                        .map(|key| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Agent(Some(key)).hash()>
                                    "Diagnose ansehen"
                                </Button>
                            }
                        })}
                    {s
                        .review
                        .map(|n| {
                            view! {
                                <Button size=ControlSize::Small href=Route::Review(Some(n)).hash()>
                                    "Review öffnen"
                                </Button>
                            }
                        })}
                    <span class="ui-spacer"></span>
                    <span class="ui-hstack t-footnote t-secondary" style="--spacing: 6px">
                        "Liegt bei" {person_avatar(&s.who)}
                        <span>{s.who.name.clone()}</span>
                    </span>
                </div>
            </div>
        </li>
    }
}

/// The entries of the chosen person.
fn person_items(store: Store, m: Member) -> impl IntoView {
    let items: Vec<Item> = store.ws.with(|w| {
        m.item_ids
            .iter()
            .filter_map(|id| w.item(id).cloned())
            .collect()
    });
    let heading = format!("Einträge von {}", m.person.name);
    let body = if items.is_empty() {
        view! { <p class="t-subheadline t-secondary">{format!("{} hat nichts offen.", m.person.name)}</p> }
            .into_any()
    } else {
        view! {
            <ul class="ck-stuck-list">
                {items
                    .into_iter()
                    .map(|item| {
                        let id = item.id.clone();
                        let (state, tone) = team::state_label(&item);
                        view! {
                            <li class="ck-stuck">
                                <div class="ui-vstack" style="--spacing: 6px; min-width: 0; flex: 1 1 auto">
                                    <div class="ui-hstack" style="--spacing: 6px">
                                        <Badge variant=BadgeVariant::Neutral>{item.source.label()}</Badge>
                                        <span class="t-footnote t-mono t-secondary">{item.key.clone()}</span>
                                    </div>
                                    <p class="t-headline">{item.title.clone()}</p>
                                    <div class="ui-hstack" style="--spacing: 8px">
                                        <Status tone=view::tone(tone)>{state}</Status>
                                        <span class="ui-spacer"></span>
                                        <Button size=ControlSize::Small on_click=move |_| store.open_item(&id)>
                                            "Zum Eintrag"
                                        </Button>
                                    </div>
                                </div>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        }
        .into_any()
    };
    view! {
        <section class="ui-vstack" style="--spacing: 12px" aria-label=heading>
            <h3 class="ui-list__header">{heading.clone()}</h3>
            {body}
        </section>
    }
}
