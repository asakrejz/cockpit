//! Pull request review: in the middle the diff, on the right the comments that the agent
//! prepared. Sending only happens once every suggestion has been decided.

use std::time::Duration;

use cockpit_core::{
    Action, AgentOptions, AgentRun, Checks, Decision, Level, MergeMethod, Review, RunKind,
    Suggestion, SuggestionDiff, Verdict,
};
use feature_ui::*;
use leptos::prelude::*;

use crate::{
    route::Route,
    sidebar::AppSidebar,
    store::{Store, use_store},
    view,
};

/// The demo agent "reads" a pull request for this long.
const REVIEW_TIME: Duration = Duration::from_millis(1200);

#[component]
pub fn ReviewPage(number: Option<u32>) -> impl IntoView {
    let store = use_store();
    // Without a number: the first review that has not been sent yet.
    let number = number.or_else(|| {
        store.ws.with_untracked(|w| {
            w.reviews
                .iter()
                .find(|r| !r.sent)
                .or(w.reviews.first())
                .map(|r| r.number)
        })
    });
    let review = Memo::new(move |_| number.and_then(|n| store.ws.with(|w| w.review(n).cloned())));

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

    let list = move || {
        store.ws.with(|w| {
            w.reviews
                .iter()
                .map(|r| {
                    view! {
                        <SidebarItem
                            href=Route::Review(Some(r.number)).hash()
                            label=format!("#{} {}", r.number, r.title)
                            current=Some(r.number) == number
                            count=if r.sent { "gesendet" } else { "" }
                        />
                    }
                })
                .collect_view()
        })
    };
    let no_reviews = move || store.ws.with(|w| w.reviews.is_empty());

    view! {
        <SplitView layout=SplitLayout::DetailWide>
            <AppSidebar>
                <SidebarSection label="Reviews" role=SectionRole::Picker>
                    {list}
                    <Show when=no_reviews>
                        <p class="ck-empty t-footnote t-secondary">"Noch keine Pull Requests zum Prüfen."</p>
                    </Show>
                </SidebarSection>
            </AppSidebar>
            {move || match review.get() {
                Some(r) => view! { {diff_pane(store, r.clone(), lines)} {agent_pane(store, r, panel)} }.into_any(),
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
mod merge;
mod suggestions;

use checks::checks_section;
use comments::comments_section;
use diff::diff_pane;
use merge::merge_section;
use suggestions::agent_pane;
