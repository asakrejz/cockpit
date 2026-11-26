//! The left list: everybody, and one row per person with numbers and workload.

use cockpit_core::{Member, team};
use feature_ui::*;
use leptos::prelude::*;

use super::labels::{counts, load_label};
use super::person_avatar;
use crate::{route::Route, store::navigate};

/// A person in the left list: numbers by status and the workload.
pub(super) fn member_row(m: Member, selected: bool) -> impl IntoView {
    let key = m.key.clone();
    let total = m.total();
    let level = m.load_level();
    let summary = t!(
        OverviewCapacity,
        name = m.person.name_in(lang()),
        used = total.min(team::CAPACITY),
        total = team::CAPACITY
    );
    view! {
        <InboxRow selected=selected on_select=move |_| {
            navigate(&Route::OverviewMember(key.clone()));
        }>
            {person_avatar(&m.person)}
            <span class="ui-vstack ui-gap-6 ui-grow">
                <span class="ui-hstack ui-gap-8 ui-flex-nowrap">
                    <span class="t-headline ck-ellipsis ui-grow">{m.person.name_in(lang())}</span>
                    <Badge variant=BadgeVariant::Neutral>{total.to_string()}</Badge>
                </span>
                {m
                    .github
                    .clone()
                    .map(|name| {
                        view! { <span class="t-footnote t-secondary ck-ellipsis">{t!(OverviewGithubName, name = name)}</span> }
                    })}
                <span class="t-subheadline t-secondary">{counts(&m)}</span>
                <Progress value=total.min(team::CAPACITY) max=team::CAPACITY label=summary />
                <span class="t-footnote t-secondary">{load_label(level)}</span>
            </span>
        </InboxRow>
    }
}

/// The first row: everybody. Chosen when no person is.
pub(super) fn everyone_row(total: usize, selected: bool) -> impl IntoView {
    view! {
        <InboxRow selected=selected on_select=move |_| navigate(&Route::OverviewAll)>
            <span class="ui-vstack ui-gap-6 ui-grow">
                <span class="ui-hstack ui-gap-8 ui-flex-nowrap">
                    <span class="t-headline ck-ellipsis ui-grow">{t!(OverviewEveryone)}</span>
                    <Badge variant=BadgeVariant::Neutral>{total.to_string()}</Badge>
                </span>
                <span class="t-subheadline t-secondary">{t!(OverviewEveryoneSummary)}</span>
            </span>
        </InboxRow>
    }
}
