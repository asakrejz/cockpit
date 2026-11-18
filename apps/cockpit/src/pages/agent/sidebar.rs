//! The list of runs in the sidebar.

use crate::route::Route;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// Runs for the sidebar. The chosen ticket is included even if it has no run yet.
pub(super) fn sidebar_runs(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        store, key, item, ..
    } = ctx;
    let current = key.get_value();
    let mut rows: Vec<(String, String, String)> = store.ws.with(|w| {
        w.runs
            .iter()
            .map(|r| {
                let state = if r.is_awaiting() || r.is_diagnosed() {
                    t!(AgentSidebarWaiting)
                } else if r.is_asking() {
                    t!(AgentSidebarAsking)
                } else if r.is_paused() {
                    t!(AgentSidebarPaused)
                } else if r.is_stopped() {
                    t!(AgentSidebarHalted)
                } else if r.is_running() {
                    t!(AgentSidebarRunning)
                } else if r.is_done() {
                    t!(AgentSidebarDone)
                } else {
                    t!(AgentSidebarReady)
                };
                let title = if r.is_followup() {
                    super::labels::run_label(r)
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
        rows.insert(0, (item.key, item.title, t!(AgentSidebarNew)));
    }
    rows.into_iter()
        .map(|(k, title, state)| {
            view! {
                <SidebarItem
                    href=Route::Agent(Some(k.clone())).hash()
                    label=[k.as_str(), title.as_str()].join(" ")
                    current=k == current
                    count=state
                />
            }
        })
        .collect_view()
}
