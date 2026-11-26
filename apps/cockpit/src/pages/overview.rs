//! Overview: on the left who works on what and how full they are; on the right what is stuck, and
//! the entries of the chosen person. The rules for it are in `cockpit_core::team`.

use cockpit_core::{Action, PeopleSource, Person, team};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    pages::team_picker::TeamPicker,
    route::Route,
    store::{Store, use_store},
};

mod entries;
mod labels;
mod link;
mod members;
mod stuck;

use entries::person_items;
use labels::group_label;
use members::{everyone_row, member_row};
use stuck::{empty_state, stuck_row};

/// The segmented control „Alle / Jira / GitHub“. It is `Action::SetPeopleSource`, kept in the
/// workspace like the other filters.
fn source_picker(store: Store) -> impl IntoView {
    let current = move || store.ws.with(|w| w.people_source);
    let label = |source: PeopleSource| match source {
        PeopleSource::All => t!(OverviewSourceAll),
        PeopleSource::Jira => t!(OverviewSourceJira),
        PeopleSource::GitHub => t!(OverviewSourceGithub),
    };
    view! {
        <div class="ui-segmented ui-self-start" role="group" aria-label=t!(OverviewSourceLabel)>
            {PeopleSource::ALL
                .into_iter()
                .map(|source| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (current() == source).to_string()
                            on:click=move |_| {
                                if let Err(e) = store.dispatch(Action::SetPeopleSource { source })
                                    && e != cockpit_core::ActionError::Unchanged
                                {
                                    store.notify(e.msg().render(feature_ui::lang_untracked()));
                                }
                            }
                        >
                            {label(source)}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

fn person_avatar(person: &Person) -> impl IntoView + use<> {
    view! {
        <Avatar
            initials=person.initials_in(lang())
            agent=person.is_agent()
            title=person.name_in(lang())
        />
    }
}

#[component]
pub fn OverviewPage() -> impl IntoView {
    let store = use_store();
    let members = Memo::new(move |_| store.ws.with(team::members));
    let stuck = Memo::new(move |_| store.ws.with(team::stuck));

    // The address chooses the person (`#/overview/Ada`); without one, or with „Alle“, everybody
    // is shown. On a narrow screen a choice also opens the detail level.
    let level = Signal::derive(move || store.route.with(Route::level));
    let chosen = Memo::new(move |_| match store.route.get() {
        Route::OverviewMember(key) => Some(key),
        _ => None,
    });
    // `None`: everybody (also if the chosen person has nothing open any more).
    let current = Memo::new(move |_| {
        let name = chosen.get();
        members.with(|list| team::chosen(list, name.as_deref()).cloned())
    });
    let stuck_shown = Memo::new(move |_| team::stuck_of(stuck.get(), current.get().as_ref()));

    view! {
        <SplitView level=level back_label=t!(OverviewTitle) back_href=Route::Overview.hash()>

            <Pane label=t!(OverviewTeam)>
                <PaneHeader>
                    <h1 class="t-large-title t-bold">{t!(OverviewTitle)}</h1>
                    <p class="t-subheadline t-secondary">
                        {t!(OverviewSubtitle)}
                    </p>
                    <TeamPicker />
                    {source_picker(store)}
                </PaneHeader>
                <ul class="ck-inbox">
                    {move || {
                        let active = current.get().map(|m| m.key);
                        let all = store.ws.with(|w| members.with(|l| team::entry_ids(w, l, None))).len();
                        let list = members.get();
                        let sections = store.ws.with(|w| {
                            team::sections(w, &list)
                                .into_iter()
                                .map(|(group, people)| {
                                    (group, people.into_iter().cloned().collect::<Vec<_>>())
                                })
                                .collect::<Vec<_>>()
                        });
                        let rows = sections
                            .into_iter()
                            .map(|(group, people)| {
                                let heading = group.map(group_label);
                                let rows = people
                                    .into_iter()
                                    .map(|m| {
                                        let selected = active.as_deref() == Some(m.key.as_str());
                                        member_row(m, selected)
                                    })
                                    .collect_view();
                                view! {
                                    {heading
                                        .map(|h| {
                                            view! { <li class="ui-list__header t-footnote t-secondary">{h}</li> }
                                        })}
                                    {rows}
                                }
                            })
                            .collect_view();
                        view! { {everyone_row(all, active.is_none())} {rows} }
                    }}
                </ul>
            </Pane>

            <Pane label=t!(OverviewDetails)>
                <PaneHeader>
                    <div class="ui-hstack ui-justify-between ui-flex-nowrap">
                        <h2 class="t-title1 t-bold">{t!(OverviewStuck)}</h2>
                        <Badge variant=BadgeVariant::Count>{move || stuck_shown.get().len().to_string()}</Badge>
                    </div>
                </PaneHeader>
                <PaneBody>
                    {move || {
                        let list = stuck_shown.get();
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
                    {move || person_items(store, current.get(), members.get())}
                </PaneBody>
            </Pane>
        </SplitView>
    }
}
