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

    // Whose pull requests the list shows; starts with those that concern me.
    let owner = RwSignal::new(Ownership::default().id().to_string());
    let owner_options: Vec<(String, String)> = Ownership::OPTIONS
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect();
    let list = move || {
        let ownership = Ownership::from_id(&owner.get()).unwrap_or_default();
        store.ws.with(|w| {
            // With reviews from more than one repo the list names the repo.
            let multi_repo = {
                let mut repos = w.reviews.iter().map(|r| r.repo.as_str());
                repos.next().is_some_and(|first| repos.any(|r| r != first))
            };
            let current = number.get_value();
            w.reviews_for(ownership, current.as_ref())
                .into_iter()
                .map(|r| {
                    view! {
                        <SidebarItem
                            href=Route::Review(Some(r.key())).hash()
                            label=review_label(r, multi_repo)
                            current=number.get_value().is_some_and(|n| n.same(&r.key()))
                            count=if r.sent { "gesendet" } else { "" }
                        />
                    }
                })
                .collect_view()
        })
    };
    let no_reviews = move || store.ws.with(|w| w.reviews.is_empty());

    view! {
        <SplitView
            layout=SplitLayout::DetailWide
            level=level
            back_label="Reviews"
            back_href=Route::Review(None).hash()
        >
            <PickerPane label="Pull Requests" title="Reviews">
                <Show when=move || store.ws.with(|w| w.reviews.iter().any(|r| !w.review_is_mine(r)))>
                    <Select
                        options=owner_options.clone()
                        value=owner
                        on_change=move |v: String| owner.set(v)
                        aria_label="Besitz"
                        compact=true
                    />
                </Show>
                <ul class="ui-sidebar__nav">{list}</ul>
                <Show when=no_reviews>
                    <p class="ck-empty t-footnote t-secondary">"Noch keine Pull Requests zum Prüfen."</p>
                </Show>
            </PickerPane>
            {move || match review.get() {
                Some(r) => view! { {diff_pane(store, r.clone(), lines, panel_ui)} {agent_pane(store, r, panel)} }.into_any(),
                None => {
                    view! {
                        <Pane label="Pull Request">
                            <PaneBody>
                                {if no_reviews() {
                                    view! {
                                        <EmptyState
                                            title="Noch keine Pull Requests"
                                            text="Sobald GitHub verbunden ist und Pull Requests auf dich warten, kannst du sie hier prüfen."
                                            icon=icons::GIT_PR
                                        >
                                            <Button size=ControlSize::Small variant=ButtonStyle::GlassProminent href=Route::Setup.hash()>
                                                "Einrichten"
                                            </Button>
                                        </EmptyState>
                                    }
                                        .into_any()
                                } else {
                                    view! {
                                        <EmptyState
                                            title="Kein Pull Request"
                                            text="Diesen Pull Request gibt es nicht (mehr). Wähle links einen anderen oder öffne ihn aus dem Posteingang."
                                            icon=icons::GIT_PR
                                        >
                                            <Button size=ControlSize::Small href=Route::Inbox(cockpit_core::Filter::All).hash()>
                                                "Zum Posteingang"
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
mod followups;
mod merge;
mod panel;
mod suggestions;

use checks::checks_section;
use comments::comments_section;
use diff::diff_pane;
use drafts::{code_rows, draft_notes, location_label};
use followups::followup_section;
use merge::merge_section;
use panel::{PanelUi, panel_body};
use suggestions::agent_pane;
