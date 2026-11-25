//! „Entfernen“ and „Alle beendeten entfernen“: take runs in a final state out of the local list.
//! Only the list of this computer changes – never a branch or pull request on GitHub. Runs that
//! work or wait cannot be removed; the page says they have to be cancelled first.

use cockpit_core::Action;
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;
use super::texts as t;

/// The button „Entfernen“ of the shown run (with a confirmation), or – for a run that still
/// works or waits – the hint that it has to be cancelled first. Nothing for a run that has not
/// started yet.
#[component]
pub(super) fn RemoveRun(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx { store, key, .. } = ctx;
    let open = RwSignal::new(false);
    // None: not in the list; Some(true): may go; Some(false): still active.
    let state = Memo::new(move |_| {
        let k = key.get_value();
        store.ws.with(|w| {
            w.runs
                .iter()
                .find(|r| r.key() == k)
                .map(|r| r.is_removable())
        })
    });
    let label = move || {
        let k = key.get_value();
        store.ws.with(|w| {
            w.runs
                .iter()
                .find(|r| r.key() == k)
                .map(super::labels::run_label)
                .unwrap_or(k)
        })
    };
    let remove = move |_| {
        let _ = store.dispatch(Action::RemoveAgentRun {
            run: key.get_value(),
        });
        open.set(false);
    };
    let footer = move || {
        view! {
            <Button on_click=move |_| open.set(false)>{t::cancel()}</Button>
            <Button variant=ButtonStyle::GlassProminent destructive=true on_click=remove>
                {t::remove_confirm()}
            </Button>
        }
    };
    move || match state.get() {
        Some(true) => view! {
            <div class="ui-hstack ui-gap-12">
                <Button on_click=move |_| open.set(true)>{t::remove_button()}</Button>
            </div>
            <Dialog open=open title=t::remove_title() footer=footer>
                <p class="t-subheadline">{move || t::remove_question(&label())}</p>
                <p class="t-footnote t-secondary">{t::remove_scope()}</p>
            </Dialog>
        }
        .into_any(),
        Some(false) => {
            view! { <p class="ui-hint" role="status">{t::remove_active_hint()}</p> }.into_any()
        }
        None => ().into_any(),
    }
}

/// „Alle beendeten entfernen (n)“ in the list of runs. Hidden while nothing can be removed.
#[component]
pub(super) fn RemoveFinished(ctx: AgentCtx) -> impl IntoView {
    let store = ctx.store;
    let open = RwSignal::new(false);
    let count = Memo::new(move |_| {
        store
            .ws
            .with(|w| w.runs.iter().filter(|r| r.is_removable()).count())
    });
    let remove = move |_| {
        let _ = store.dispatch(Action::RemoveFinishedRuns);
        open.set(false);
    };
    let footer = move || {
        view! {
            <Button on_click=move |_| open.set(false)>{t::cancel()}</Button>
            <Button variant=ButtonStyle::GlassProminent destructive=true on_click=remove>
                {t::remove_confirm()}
            </Button>
        }
    };
    move || {
        (count.get() > 0).then(|| {
            view! {
                <div class="ck-cleanup">
                    <Button on_click=move |_| open.set(true)>
                        {move || t::remove_all_button(count.get())}
                    </Button>
                </div>
                <Dialog open=open title=t::remove_all_title() footer=footer>
                    <p class="t-subheadline">{move || t::remove_all_question(count.get())}</p>
                    <p class="t-footnote t-secondary">{t::remove_all_keep()}</p>
                    <p class="t-footnote t-secondary">{t::remove_scope()}</p>
                </Dialog>
            }
        })
    }
}
