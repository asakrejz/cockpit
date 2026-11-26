//! "Is the same person as …": linking a Jira person to a GitHub user.

use cockpit_core::{
    Action, Member, people_search,
    team::{self, Side},
};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    route::Route,
    store::{Store, navigate},
};

/// "Is the same person as …": link a Jira person to a GitHub user (or the other way round), or
/// remove the link. Nothing is guessed; the link is `Action::LinkPeople`.
pub(super) fn link_section(store: Store, m: Member) -> impl IntoView {
    if m.is_linked() {
        let jira = m.person.name.clone();
        let github = m.github.clone().unwrap_or_default();
        return view! {
            <div class="ui-vstack ui-gap-8">
                <p class="t-subheadline t-secondary">{t!(OverviewLinkedWith, name = github)}</p>
                <div>
                    <Button
                        size=ControlSize::Small
                        on_click=move |_| {
                            let _ = store.dispatch(Action::UnlinkPeople { jira: jira.clone() });
                            navigate(&Route::OverviewAll);
                        }
                    >
                        {t!(OverviewUnlink)}
                    </Button>
                </div>
            </div>
        }
        .into_any();
    }
    let Some(side) = m.unlinked_side() else {
        return ().into_any();
    };
    let me = m.person.name.clone();
    let query = RwSignal::new(String::new());
    let found = Memo::new(move |_| {
        let local = store.ws.with(|w| w.link_candidates(side));
        people_search::search(&local, &[], &query.get())
    });
    let options = Signal::derive(move || {
        found
            .get()
            .into_iter()
            .map(|p| ComboOption::new(p.name.clone(), p.name).initials(p.initials))
            .collect::<Vec<_>>()
    });
    let on_select = move |other: String| {
        let (jira, github) = match side {
            Side::Jira => (me.clone(), other.clone()),
            Side::GitHub => (other.clone(), me.clone()),
        };
        let jira_name = jira.clone();
        match store.dispatch(Action::LinkPeople { jira, github }) {
            Ok(_) => {
                store.notify(t!(OverviewLinkDone, name = other));
                // The person is now the linked member: stay on them.
                navigate(&Route::OverviewMember(team::linked_key(&jira_name)));
            }
            Err(e) => store.notify(e.msg().render(feature_ui::lang_untracked())),
        }
    };
    let hint = match side {
        Side::Jira => t!(OverviewLinkHintJira),
        Side::GitHub => t!(OverviewLinkHintGithub),
    };
    view! {
        <div class="ui-vstack ui-gap-8">
            <h4 class="t-headline">{t!(OverviewLinkTitle)}</h4>
            <p class="t-footnote t-secondary">{hint}</p>
            <Combobox
                label=t!(OverviewLinkLabel)
                label_hidden=true
                value=Signal::derive(String::new)
                query=query
                options=options
                on_select=on_select
                placeholder=t!(OverviewLinkPlaceholder)
                empty_text=t!(OverviewLinkEmpty)
            />
        </div>
    }
    .into_any()
}
