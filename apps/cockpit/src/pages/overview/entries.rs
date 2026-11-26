//! The entries of the chosen person.

use cockpit_core::{Item, Member, team};
use feature_ui::*;
use leptos::prelude::*;

use super::labels::state_text;
use super::link::link_section;
use crate::{store::Store, view};

/// The entries of the chosen person.
pub(super) fn person_items(
    store: Store,
    member: Option<Member>,
    all: Vec<Member>,
) -> impl IntoView {
    let items: Vec<Item> = store.ws.with(|w| {
        team::entry_ids(w, &all, member.as_ref())
            .iter()
            .filter_map(|id| w.item(id).cloned())
            .collect()
    });
    let (heading, nothing) = match &member {
        Some(m) => (
            t!(OverviewEntriesOf, name = m.person.name_in(lang())),
            t!(OverviewNothingOpenFor, name = m.person.name_in(lang())),
        ),
        None => (t!(OverviewEntriesAll), t!(OverviewNothingOpen)),
    };
    let link = member.clone().map(|m| link_section(store, m));
    let body = if items.is_empty() {
        view! { <p class="t-subheadline t-secondary">{nothing}</p> }.into_any()
    } else {
        view! {
            <ul class="ck-stuck-list">
                {items
                    .into_iter()
                    .map(|item| {
                        let id = item.id.clone();
                        let tone = team::state_tone(&item);
                        let state = state_text(&item);
                        view! {
                            <li class="ck-stuck">
                                <div class="ui-vstack ui-gap-6 ui-grow">
                                    <div class="ui-hstack ui-gap-6">
                                        <Badge variant=BadgeVariant::Neutral>{item.source.label()}</Badge>
                                        <span class="t-footnote t-mono t-secondary">{item.key.clone()}</span>
                                    </div>
                                    <p class="t-headline">{item.title.clone()}</p>
                                    <div class="ui-hstack ui-gap-8">
                                        <Status tone=view::tone(tone)>{state}</Status>
                                        <span class="ui-spacer"></span>
                                        <Button size=ControlSize::Small on_click=move |_| store.open_item(&id)>
                                            {t!(OverviewToEntry)}
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
        <section class="ui-vstack ui-gap-12" aria-label=heading>
            <h3 class="ui-list__header">{heading.clone()}</h3>
            {link}
            {body}
        </section>
    }
}
