//! Pull-Request-Review: in der Mitte der Diff, rechts die Kommentare, die der Agent
//! vorbereitet hat. Gesendet wird erst, wenn über jeden Vorschlag entschieden ist.

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

/// So lange „liest“ der Demo-Agent einen Pull Request.
const REVIEW_TIME: Duration = Duration::from_millis(1200);

#[component]
pub fn ReviewPage(number: Option<u32>) -> impl IntoView {
    let store = use_store();
    // Ohne Nummer: das erste Review, das noch nicht gesendet ist.
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

    // Zustand der Zeilenkommentare im Diff.
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

    // Zustand der rechten Spalte. Er überlebt, wenn sich das Review ändert.
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

    view! {
        <SplitView layout=SplitLayout::DetailWide>
            <AppSidebar>
                <SidebarSection label="Reviews">{list}</SidebarSection>
            </AppSidebar>
            {move || match review.get() {
                Some(r) => view! { {diff_pane(store, r.clone(), lines)} {agent_pane(store, r, panel)} }.into_any(),
                None => {
                    view! {
                        <Pane label="Pull Request">
                            <PaneHeader>
                                <h1 class="t-title1 t-bold">"Kein Pull Request"</h1>
                                <p class="t-subheadline t-secondary">
                                    "Diesen Pull Request gibt es nicht. Wähle links einen anderen."
                                </p>
                            </PaneHeader>
                        </Pane>
                    }
                        .into_any()
                }
            }}
        </SplitView>
    }
}

/// Eingaben der rechten Spalte.
#[derive(Clone, Copy)]
struct Panel {
    own_comment: RwSignal<String>,
    /// ID des Vorschlags, dessen Text gerade bearbeitet wird.
    editing: RwSignal<Option<String>>,
    edit_text: RwSignal<String>,
    agent_reading: RwSignal<bool>,
    /// Gewählte Merge-Methode (ID aus `MergeMethod::OPTIONS`).
    merge_method: RwSignal<&'static str>,
    /// Name des Checks, dessen Log aufgeklappt ist.
    open_check: RwSignal<Option<String>>,
    /// IDs der Kommentare, die der Agent nicht umsetzen soll. Standard: alle offenen.
    deselected: RwSignal<Vec<String>>,
}

/// Eingaben der Zeilenkommentare im Diff.
#[derive(Clone, Copy)]
struct LineUi {
    /// Datei und Position der Zeile, an der gerade ein Kommentar entsteht.
    open: RwSignal<Option<(String, usize)>>,
    draft: RwSignal<String>,
    /// ID des eigenen Kommentars, der gerade bearbeitet wird.
    editing: RwSignal<Option<String>>,
    edit_text: RwSignal<String>,
    /// Schlägt der neue Kommentar eine Änderung vor? Dann steht der Ersatz in `code`.
    suggest: RwSignal<bool>,
    code: RwSignal<String>,
    /// Dasselbe für den Kommentar, der gerade bearbeitet wird.
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
