//! The list of runs in the sidebar.

use crate::route::Route;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;
use super::texts as t;

/// The switch „Erledigte anzeigen“ with the number of finished runs. Hidden while there are none.
pub(super) fn closed_switch(ctx: AgentCtx, show: RwSignal<bool>) -> impl IntoView {
    let store = ctx.store;
    let count = Memo::new(move |_| {
        store
            .ws
            .with(|w| w.runs.iter().filter(|r| r.is_closed()).count())
    });
    move || {
        (count.get() > 0).then(|| {
            view! {
                <div class="ck-rule">
                    <label class="t-subheadline">
                        <span>{move || t::show_closed_count(count.get())}</span>
                        <Toggle checked=show aria_label=t::SHOW_CLOSED />
                    </label>
                </div>
            }
        })
    }
}

/// Runs for the sidebar. The chosen ticket is included even if it has no run yet; finished
/// ones (merged or closed pull request) only with `show_closed` – or when one of them is open.
pub(super) fn sidebar_runs(ctx: AgentCtx, show_closed: bool) -> impl IntoView {
    let AgentCtx {
        store, key, item, ..
    } = ctx;
    let current = key.get_value();
    let mut rows: Vec<(String, String, &'static str)> = store.ws.with(|w| {
        w.runs
            .iter()
            .filter(|r| show_closed || !r.is_closed() || r.key() == current)
            .map(|r| {
                let state = if r.is_closed() {
                    t::CLOSED_BADGE
                } else if r.is_awaiting() || r.is_diagnosed() {
                    "wartet"
                } else if r.is_asking() {
                    "fragt"
                } else if r.is_paused() {
                    "pausiert"
                } else if r.is_stopped() {
                    "angehalten"
                } else if r.is_running() {
                    "läuft"
                } else if r.is_done() {
                    "fertig"
                } else {
                    "bereit"
                };
                let title = if r.is_followup() {
                    r.label()
                } else {
                    r.ticket_title.clone()
                };
                (r.key(), title, state)
            })
            .collect()
    });
    if !rows.iter().any(|(k, _, _)| *k == current)
        && let Some(item) = item.get()
    {
        rows.insert(0, (item.key, item.title, "neu"));
    }
    rows.into_iter()
        .map(|(k, title, state)| {
            view! {
                <SidebarItem
                    href=Route::Agent(Some(k.clone())).hash()
                    label=format!("{k} {title}")
                    current=k == current
                    count=state
                />
            }
        })
        .collect_view()
}
