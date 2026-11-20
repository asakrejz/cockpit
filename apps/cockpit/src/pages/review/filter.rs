//! The filter of the Reviews list: whose pull requests, which repos (grouped by owner, with
//! counts and a search) and, when GitHub Projects are connected, which projects. A display
//! filter only (`Action::SetReviewFilter`): the sync loads every pull request that concerns me,
//! whatever the filter says. Nothing chosen means "all".

use cockpit_core::{Action, ActionError, Applied, Ownership, ReviewFilter};
use feature_ui::*;
use leptos::prelude::*;

use crate::pages::inbox::picker::SearchBox;
use crate::store::Store;

/// With more repos than this the dialog shows a search field.
const SEARCH_FROM: usize = 8;

fn report(store: Store, result: Result<Applied, ActionError>) {
    if let Err(e) = result
        && e != ActionError::Unchanged
    {
        store.notify(e.msg().render(lang_untracked()));
    }
}

/// How many things the filter narrows (repos plus projects, and "mine" counts as one).
fn active_count(f: &ReviewFilter) -> usize {
    f.repos.len() + f.projects.len() + usize::from(f.ownership == Ownership::Mine)
}

/// The button "Filter" with the number of active choices; opens the dialog.
pub(super) fn filter_button(store: Store, open: RwSignal<bool>) -> impl IntoView {
    let count = Signal::derive(move || store.ws.with(|w| active_count(&w.review_filter)));
    view! {
        <Button size=ControlSize::Small on_click=move |_| open.set(true) aria_label=t!(ReviewFilterOpen)>
            {t!(ReviewFilterButton)}
            <Show when=move || { count.get() > 0 }>
                <Badge variant=BadgeVariant::Count>{move || count.get().to_string()}</Badge>
            </Show>
        </Button>
    }
}

/// One line under the button that says what is shown, with a link to show everything.
pub(super) fn filter_summary(store: Store) -> impl IntoView {
    move || {
        let f = store.ws.with(|w| w.review_filter.clone());
        (f.narrows()).then(|| {
            let text = match (f.repos.len(), f.projects.len()) {
                (r, 0) => t!(ReviewFilterSummaryRepos, n = r),
                (0, p) => t!(ReviewFilterSummaryProjects, n = p),
                (r, p) => t!(ReviewFilterSummaryBoth, repos = r, projects = p),
            };
            let clear = move |_| {
                let mut f = store.ws.with_untracked(|w| w.review_filter.clone());
                f.repos.clear();
                f.projects.clear();
                report(store, store.dispatch(Action::SetReviewFilter { filter: f }));
            };
            view! {
                <p class="t-footnote t-secondary">
                    {text} " "
                    <Button variant=ButtonStyle::Borderless size=ControlSize::Small on_click=clear>{t!(ReviewFilterShowAll)}</Button>
                </p>
            }
        })
    }
}

/// The dialog. Edits go to a draft; "Anwenden" applies and stores it.
pub(super) fn filter_dialog(store: Store, open: RwSignal<bool>) -> impl IntoView {
    let draft = RwSignal::new(ReviewFilter::default());
    let query = RwSignal::new(String::new());
    // Every time the dialog opens it starts from what is applied.
    Effect::new(move |_| {
        if open.get() {
            draft.set(store.ws.with_untracked(|w| w.review_filter.clone()));
            query.set(String::new());
        }
    });
    let total_repos = move || {
        store.ws.with(|w| {
            w.review_repo_groups(draft.with(|d| d.ownership), "")
                .iter()
                .map(|g| g.repos.len())
                .sum::<usize>()
        })
    };
    let groups = move || {
        let ownership = draft.with(|d| d.ownership);
        let q = query.get();
        store.ws.with(|w| w.review_repo_groups(ownership, &q))
    };
    let projects = move || store.ws.with(|w| w.review_projects());

    let apply = move |_| {
        let filter = draft.get_untracked();
        report(store, store.dispatch(Action::SetReviewFilter { filter }));
        open.set(false);
    };
    let reset = move |_| draft.set(ReviewFilter::default());

    let body = move || {
        let owner_options: Vec<(&'static str, String)> =
            vec![("mine", t!(ReviewOwnerMine)), ("all", t!(ReviewOwnerAll))];
        view! {
            <div class="ck-dialog">
                <div class="ck-dialog__field">
                    <span class="ui-field__label">{t!(ReviewOwnerLabel)}</span>
                    {super::segmented(
                        owner_options,
                        move || draft.with(|d| d.ownership.id()),
                        move |id| {
                            draft.update(|d| d.ownership = Ownership::from_id(id).unwrap_or_default())
                        },
                        t!(ReviewOwnerLabel),
                        false,
                    )}
                </div>
                <h2 class="t-headline ck-dialog__heading">{t!(ReviewFilterRepos)}</h2>
                <p class="t-footnote t-secondary">{t!(ReviewFilterAllHint)}</p>
                <Show when=move || { total_repos() > SEARCH_FROM || !query.with(String::is_empty) }>
                    <SearchBox value=query placeholder=t!(ReviewFilterSearch) />
                </Show>
                <div class="ck-filter-groups">
                    {move || {
                        let list = groups();
                        if list.is_empty() {
                            return view! { <p class="t-footnote t-secondary">{t!(ReviewFilterNoRepos)}</p> }
                                .into_any();
                        }
                        list.into_iter()
                            .map(|g| {
                                let names: Vec<String> = g.repos.iter().map(|(r, _)| r.clone()).collect();
                                let all_on = {
                                    let names = names.clone();
                                    move || draft.with(|d| names.iter().all(|n| d.repos.iter().any(|r| r.eq_ignore_ascii_case(n))))
                                };
                                let toggle_owner = {
                                    let names = names.clone();
                                    move |_| {
                                        let on = draft.with_untracked(|d| names.iter().all(|n| d.repos.iter().any(|r| r.eq_ignore_ascii_case(n))));
                                        draft.update(|d| {
                                            for n in &names {
                                                let has = d.repos.iter().any(|r| r.eq_ignore_ascii_case(n));
                                                if on == has {
                                                    d.toggle_repo(n);
                                                }
                                            }
                                        });
                                    }
                                };
                                view! {
                                    <section class="ck-filter-group">
                                        <label class="ck-repo-item ck-filter-owner">
                                            <input type="checkbox" prop:checked=all_on on:change=toggle_owner />
                                            <span class="ck-repo-name">{g.owner.clone()}</span>
                                            <span class="t-footnote t-secondary">{g.count().to_string()}</span>
                                        </label>
                                        <ul class="ck-repo-list">
                                            {g.repos
                                                .into_iter()
                                                .map(|(repo, n)| {
                                                    let name = repo.clone();
                                                    let on = {
                                                        let repo = repo.clone();
                                                        move || draft.with(|d| d.repos.iter().any(|r| r.eq_ignore_ascii_case(&repo)))
                                                    };
                                                    view! {
                                                        <li>
                                                            <label class="ck-repo-item">
                                                                <input
                                                                    type="checkbox"
                                                                    prop:checked=on
                                                                    on:change=move |_| draft.update(|d| d.toggle_repo(&name))
                                                                />
                                                                <span class="ck-repo-name">{repo.clone()}</span>
                                                                <span class="t-footnote t-secondary">{n.to_string()}</span>
                                                            </label>
                                                        </li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>
                                    </section>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </div>
                <Show when=move || !projects().is_empty()>
                    <h2 class="t-headline ck-dialog__heading">{t!(ReviewFilterProjects)}</h2>
                    <p class="t-footnote t-secondary">{t!(ReviewFilterProjectsHint)}</p>
                    <ul class="ck-repo-list">
                        {move || {
                            projects()
                                .into_iter()
                                .map(|p| {
                                    let id = p.id.clone();
                                    let on = {
                                        let id = id.clone();
                                        move || draft.with(|d| d.projects.contains(&id))
                                    };
                                    view! {
                                        <li>
                                            <label class="ck-repo-item">
                                                <input
                                                    type="checkbox"
                                                    prop:checked=on
                                                    on:change=move |_| draft.update(|d| d.toggle_project(&id))
                                                />
                                                <span class="ck-repo-name">{p.title.clone()}</span>
                                                <span class="t-footnote t-secondary">{p.count.to_string()}</span>
                                            </label>
                                        </li>
                                    }
                                })
                                .collect_view()
                        }}
                    </ul>
                </Show>
            </div>
        }
    };
    let footer = move || {
        view! {
            <Button on_click=reset>{t!(ReviewFilterReset)}</Button>
            <Button variant=ButtonStyle::GlassProminent on_click=apply>
                {t!(ReviewFilterApply)}
            </Button>
        }
    };
    view! {
        <Dialog open=open title=t!(ReviewFilterTitle) footer=footer>
            {body}
        </Dialog>
    }
}
