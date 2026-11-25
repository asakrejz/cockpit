//! The review panel: tabs „Dateien“ (tree, diffs, „Angesehen“) and „Commits“ (list, diff of one
//! commit). The state lives in [`PanelUi`], created once per page so that it survives every
//! update of the workspace.

use cockpit_core::api::{PanelCommitDiff, PanelCommits};
use cockpit_core::{FileDiff, PrKey, SHOWN_LINES, Viewed};

use super::*;

mod commits;
mod files;
mod storage;

pub(super) use commits::commits_tab;
pub(super) use files::files_tab;

/// Something that is fetched from the server on demand.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Load<T> {
    Idle,
    Loading,
    Done(T),
    Failed(String),
}

/// State of the panel.
#[derive(Clone, Copy)]
pub(super) struct PanelUi {
    pub tab: RwSignal<&'static str>,
    /// Files the person has looked at.
    pub viewed: RwSignal<Viewed>,
    /// Files opened or closed by hand (the default: open, but closed once viewed).
    pub open: RwSignal<Vec<(String, bool)>>,
    /// Files whose whole diff is shown („Mehr anzeigen“).
    pub more: RwSignal<Vec<String>>,
    pub commits: RwSignal<Load<PanelCommits>>,
    /// The commit whose diff is shown.
    pub chosen: RwSignal<Option<String>>,
    pub commit_diff: RwSignal<Load<PanelCommitDiff>>,
    /// Files of the chosen commit that were fetched in full.
    pub full: RwSignal<Vec<cockpit_core::api::PanelFile>>,
}

impl PanelUi {
    pub fn new(key: Option<&PrKey>) -> Self {
        Self {
            tab: RwSignal::new("files"),
            viewed: RwSignal::new(key.map(storage::load).unwrap_or_default()),
            open: RwSignal::new(Vec::new()),
            more: RwSignal::new(Vec::new()),
            commits: RwSignal::new(Load::Idle),
            chosen: RwSignal::new(None),
            commit_diff: RwSignal::new(Load::Idle),
            full: RwSignal::new(Vec::new()),
        }
    }
}

/// The body of the diff pane: tab switch and the chosen tab.
pub(super) fn panel_body(store: Store, r: Review, lines: LineUi, ui: PanelUi) -> impl IntoView {
    let on_change = Callback::new(move |id: &'static str| ui.tab.set(id));
    let r = StoredValue::new(r);
    view! {
        <div class="ui-vstack ui-gap-12">
            {segmented(
                vec![("files", t!(ReviewPanelTabFiles)), ("commits", t!(ReviewPanelTabCommits))],
                move || ui.tab.get(),
                move |id| on_change.run(id),
                t!(ReviewPanelView),
                false,
            )}
            {move || {
                let r = r.get_value();
                match ui.tab.get() {
                    "commits" => commits_tab(r, ui),
                    _ => files_tab(store, r, lines, ui),
                }
            }}
        </div>
    }
}

/// Opened or closed, as the person left it.
pub(super) fn is_open(ui: PanelUi, name: &str, viewed: bool) -> bool {
    ui.open
        .with(|o| o.iter().find(|(n, _)| n == name).map(|(_, v)| *v))
        .unwrap_or(!viewed)
}

pub(super) fn set_open(ui: PanelUi, name: &str, open: bool) {
    ui.open.update(|o| {
        o.retain(|(n, _)| n != name);
        o.push((name.to_string(), open));
    });
}

/// Message for a failed load, with a retry button.
pub(super) fn failure(message: String, retry: impl Fn() + Send + Sync + 'static) -> AnyView {
    view! {
        <div class="ui-vstack ui-gap-8 ui-items-start">
            <p class="t-subheadline ck-problem">{message}</p>
            <Button size=ControlSize::Small on_click=move |_| retry()>
                {t!(ReviewPanelRetry)}
            </Button>
        </div>
    }
    .into_any()
}
