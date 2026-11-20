//! Pull request review: in the middle the diff, on the right the comments that the agent
//! prepared. Sending only happens once every suggestion has been decided.

use std::time::Duration;

use cockpit_core::{
    Action, AgentOptions, AgentRun, Checks, Decision, Level, MergeMethod, Ownership, PrKey, Review,
    RunKind, Suggestion, SuggestionDiff, Verdict,
};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    access_hint::{AccessHint, problem_of, use_github_access},
    route::Route,
    server,
    store::{Store, use_store},
    view,
};

/// The demo agent "reads" a pull request for this long.
const REVIEW_TIME: Duration = Duration::from_millis(1200);

#[component]
pub fn ReviewPage(number: Option<PrKey>) -> impl IntoView {
    let store = use_store();
    // "No pull requests" is only true if GitHub shows every configured repo.
    let (access, recheck) = use_github_access();
    // On a narrow screen the list is the level without a number in the address.
    let level = if number.is_some() {
        StackLevel::Detail
    } else {
        StackLevel::List
    };
    // Without a number: the first review that has not been sent yet.
    let number = number.or_else(|| {
        store.ws.with_untracked(|w| {
            let mine = w.reviews_for(Ownership::Mine, None);
            mine.iter()
                .find(|r| !r.sent)
                .or(mine.first())
                .copied()
                .or(w.reviews.first())
                .map(|r| r.key())
        })
    });
    let number = StoredValue::new(number);
    let review = Memo::new(move |_| {
        number
            .get_value()
            .and_then(|n| store.ws.with(|w| w.review(n).cloned()))
    });

    // State of the line comments in the diff.
    let lines = LineUi {
        open: RwSignal::new(None),
        draft: RwSignal::new(String::new()),
        editing: RwSignal::new(None),
        edit_text: RwSignal::new(String::new()),
        suggest: RwSignal::new(false),
        code: RwSignal::new(String::new()),
        edit_suggest: RwSignal::new(false),
        edit_code: RwSignal::new(String::new()),
    };

    // State of the review panel (tabs, viewed files, commits).
    let panel_ui = PanelUi::new(number.get_value().as_ref());

    // State of the right column. It survives when the review changes.
    let panel = Panel {
        own_comment: RwSignal::new(String::new()),
        editing: RwSignal::new(None),
        edit_text: RwSignal::new(String::new()),
        agent_reading: RwSignal::new(false),
        merge_method: RwSignal::new(MergeMethod::default().id()),
        open_check: RwSignal::new(None),
        deselected: RwSignal::new(Vec::new()),
    };

    // Which pull requests the list shows: the persisted filter (repos, projects, whose).
    let filter_open = RwSignal::new(false);
    let list = move || {
        store.ws.with(|w| {
            // With reviews from more than one repo the list names the repo.
            let multi_repo = {
                let mut repos = w.reviews.iter().map(|r| r.repo.as_str());
                repos.next().is_some_and(|first| repos.any(|r| r != first))
            };
            let current = number.get_value();
            w.reviews_filtered(&w.review_filter, current.as_ref())
                .into_iter()
                .map(|r| {
                    view! {
                        <SidebarItem
                            href=Route::Review(Some(r.key())).hash()
                            label=review_label(r, multi_repo)
                            current=number.get_value().is_some_and(|n| n.same(&r.key()))
                            count=if r.sent { t!(ReviewSent) } else { String::new() }
                        />
                    }
                })
                .collect_view()
        })
    };
    let filtered_out = move || {
        store.ws.with(|w| {
            !w.reviews.is_empty() && w.reviews_filtered(&w.review_filter, None).is_empty()
        })
    };
    let no_reviews = move || store.ws.with(|w| w.reviews.is_empty());

    view! {
        <SplitView
            layout=SplitLayout::DetailWide
            level=level
            back_label=t!(ReviewBack)
            back_href=Route::Review(None).hash()
        >
            <PickerPane label=t!(ReviewListLabel) title=t!(ReviewTitle)>
                <Show when=move || !no_reviews()>
                    {filter::filter_button(store, filter_open)}
                    {filter::filter_summary(store)}
                </Show>
                {filter::filter_dialog(store, filter_open)}
                <ul class="ui-sidebar__nav">{list}</ul>
                <Show when=move || filtered_out()>
                    <p class="ck-empty t-footnote t-secondary">{t!(ReviewFilterNoneTitle)}</p>
                </Show>
                <Show when=move || no_reviews() && problem_of(&access).is_none()>
                    <p class="ck-empty t-footnote t-secondary">{t!(ReviewListEmpty)}</p>
                </Show>
            </PickerPane>
            {move || match review.get() {
                Some(r) => view! { {diff_pane(store, r.clone(), lines, panel_ui)} {agent_pane(store, r, panel)} }.into_any(),
                None => {
                    view! {
                        <Pane label=t!(ReviewPane)>
                            <PaneBody>
                                {move || if no_reviews() && let Some(a) = problem_of(&access) {
                                    view! { <AccessHint access=a recheck=recheck /> }.into_any()
                                } else if no_reviews() {
                                    view! {
                                        <EmptyState
                                            title=t!(ReviewEmptyTitle)
                                            text=t!(ReviewEmptyText)
                                            icon=icons::GIT_PR
                                        >
                                            <Button size=ControlSize::Small variant=ButtonStyle::GlassProminent href=Route::Setup.hash()>
                                                {t!(ReviewSetup)}
                                            </Button>
                                        </EmptyState>
                                    }
                                        .into_any()
                                } else {
                                    view! {
                                        <EmptyState
                                            title=t!(ReviewMissingTitle)
                                            text=t!(ReviewMissingText)
                                            icon=icons::GIT_PR
                                        >
                                            <Button size=ControlSize::Small href=Route::Inbox(cockpit_core::Filter::All).hash()>
                                                {t!(ReviewToInbox)}
                                            </Button>
                                        </EmptyState>
                                    }
                                        .into_any()
                                }}
                            </PaneBody>
                        </Pane>
                    }
                        .into_any()
                }
            }}
        </SplitView>
    }
}

/// `#87 Titel`, with several repos `repo#87 Titel`.
fn review_label(r: &Review, multi_repo: bool) -> String {
    if multi_repo && !r.repo.is_empty() {
        format!("{}#{} {}", r.repo, r.number, r.title)
    } else {
        format!("#{} {}", r.number, r.title)
    }
}

/// A segmented switch with texts that are looked up at run time (the `ui` one needs `'static`
/// labels).
fn segmented(
    options: Vec<(&'static str, String)>,
    value: impl Fn() -> &'static str + Send + Sync + 'static + Copy,
    on_change: impl Fn(&'static str) + Send + Sync + 'static + Copy,
    aria_label: String,
    stretch: bool,
) -> impl IntoView {
    let align = if stretch { "stretch" } else { "flex-start" };
    let flex = stretch.then_some("1 1 0");
    let pad = stretch.then_some("8px");
    view! {
        <div class="ui-segmented" role="group" aria-label=aria_label style:align-self=align>
            {options
                .into_iter()
                .map(|(id, label)| {
                    view! {
                        <button
                            type="button"
                            style:flex=flex
                            style:padding-inline=pad
                            aria-pressed=move || (value() == id).to_string()
                            on:click=move |_| on_change(id)
                        >
                            {label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// Inputs of the right column.
#[derive(Clone, Copy)]
struct Panel {
    own_comment: RwSignal<String>,
    /// ID of the suggestion whose text is being edited right now.
    editing: RwSignal<Option<String>>,
    edit_text: RwSignal<String>,
    agent_reading: RwSignal<bool>,
    /// Chosen merge method (ID from `MergeMethod::OPTIONS`).
    merge_method: RwSignal<&'static str>,
    /// Name of the check whose log is expanded.
    open_check: RwSignal<Option<String>>,
    /// IDs of the comments that the agent should not implement. Default: all open ones.
    deselected: RwSignal<Vec<String>>,
}

/// Inputs of the line comments in the diff.
#[derive(Clone, Copy)]
struct LineUi {
    /// File and position of the line at which a comment is being written right now.
    open: RwSignal<Option<(String, usize)>>,
    draft: RwSignal<String>,
    /// ID of the own comment that is being edited right now.
    editing: RwSignal<Option<String>>,
    edit_text: RwSignal<String>,
    /// Does the new comment suggest a change? Then the replacement is in `code`.
    suggest: RwSignal<bool>,
    code: RwSignal<String>,
    /// The same for the comment that is being edited right now.
    edit_suggest: RwSignal<bool>,
    edit_code: RwSignal<String>,
}

mod checks;
mod comments;
mod diff;
mod drafts;
mod filter;
mod followups;
mod merge;
mod panel;
mod suggestions;

use checks::{checks_label, checks_section};
use comments::comments_section;
use diff::diff_pane;
use drafts::{code_rows, draft_notes, location_label};
use followups::followup_section;
use merge::merge_section;
use panel::{PanelUi, panel_body};
use suggestions::agent_pane;
