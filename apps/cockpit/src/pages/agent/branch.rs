//! „Branch löschen“ for a finished run whose pull request is merged or closed. The button is
//! only ever pressed by you; the backend checks everything again before it deletes exactly this
//! one branch. While a rule says no, the button is off and the reason stands next to it.

use cockpit_core::api::BranchCheck;
use cockpit_core::{AgentRun, RunKind};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::texts as t;
use crate::server;

/// Does the run own a branch the cockpit pushed, and is the run done with?
fn eligible(r: &AgentRun) -> bool {
    r.kind == RunKind::Implement && r.real && r.is_closed() && r.pull_request.is_some()
}

#[component]
pub(super) fn BranchPanel(run: Memo<Option<AgentRun>>) -> impl IntoView {
    let check = RwSignal::new(None::<BranchCheck>);
    let problem = RwSignal::new(None::<String>);
    let deleted = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let open = RwSignal::new(false);

    let key = Memo::new(move |_| {
        run.get()
            .filter(eligible)
            .map(|r| r.key())
            .unwrap_or_default()
    });
    // Whenever another run is shown, ask the backend again (read-only).
    Effect::new(move |_| {
        let key = key.get();
        check.set(None);
        problem.set(None);
        deleted.set(false);
        if key.is_empty() {
            return;
        }
        spawn_local(async move {
            match server::fetch_branch_check(&key).await {
                Ok(c) => check.set(Some(c)),
                Err(e) => problem.set(Some(e.message())),
            }
        });
    });
    let delete = move |_| {
        busy.set(true);
        problem.set(None);
        let key = key.get_untracked();
        spawn_local(async move {
            match server::post_branch_delete(&key).await {
                Ok(_) => {
                    deleted.set(true);
                    open.set(false);
                }
                Err(e) => problem.set(Some(e.message())),
            }
            busy.set(false);
        });
    };
    let branch = move || check.with(|c| c.as_ref().map(|c| c.branch.clone()).unwrap_or_default());
    let disabled = Signal::derive(move || {
        deleted.get() || check.with(|c| c.as_ref().is_none_or(|c| !c.allowed))
    });
    // The sentence next to the button: why it is off, or what happened.
    let note = move || {
        if deleted.get() {
            return Some(t::BRANCH_DELETED.to_string());
        }
        match check.get() {
            None if problem.get().is_none() => Some(t::BRANCH_CHECKING.to_string()),
            Some(c) => c.reason,
            None => None,
        }
    };
    let footer = move || {
        view! {
            <Button on_click=move |_| open.set(false)>{t::CANCEL}</Button>
            <Button variant=ButtonStyle::GlassProminent disabled=busy on_click=delete>
                {t::BRANCH_BUTTON}
            </Button>
        }
    };
    move || {
        (!key.get().is_empty()).then(|| {
            view! {
                <div class="ui-list__section" role="region" aria-label=t::BRANCH_TITLE>
                    <p class="ui-list__header">{t::BRANCH_TITLE}</p>
                    <p class="t-subheadline t-mono">{branch}</p>
                    <div class="ui-hstack" style="--spacing: 12px; flex-wrap: wrap">
                        <Button disabled=disabled on_click=move |_| open.set(true)>
                            {t::BRANCH_BUTTON}
                        </Button>
                    </div>
                    {move || {
                        note()
                            .map(|m| view! { <p class="ui-hint" role="status">{m}</p> })
                    }}
                    {move || problem.get().map(|m| view! { <p class="ui-hint ck-problem" role="alert">{m}</p> })}
                    <Dialog open=open title=t::BRANCH_DIALOG_TITLE footer=footer>
                        {move || {
                            view! {
                                <p class="t-subheadline">{t::branch_question(&branch())}</p>
                                <p class="t-footnote t-secondary">{t::BRANCH_FINAL}</p>
                            }
                        }}
                    </Dialog>
                </div>
            }
        })
    }
}
