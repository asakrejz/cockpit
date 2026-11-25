//! The repos of a run in several repos: for each one its branch, its state and its pull request.

use cockpit_core::{AgentRun, RunKind, RunRepoState};
use feature_ui::*;
use leptos::prelude::*;

fn tone(state: RunRepoState) -> StatusTone {
    match state {
        RunRepoState::Pending => StatusTone::Gray,
        RunRepoState::Unchanged => StatusTone::Gray,
        RunRepoState::Pushed => StatusTone::Blue,
        RunRepoState::PrCreated => StatusTone::Green,
        RunRepoState::Refused => StatusTone::Red,
    }
}

#[component]
pub(super) fn RunRepos(run: Memo<Option<AgentRun>>) -> impl IntoView {
    move || {
        let r = run
            .get()
            .filter(|r| r.kind == RunKind::Implement && r.is_multi_repo() && !r.is_idle())?;
        let rows = r
            .repos
            .iter()
            .map(|e| {
                let branch = r.branch_in(&e.repo);
                let shown = e.is_pushed().then_some(branch);
                let pull = e.pull_request.clone();
                let lang = lang_untracked();
                let state = e.state_msg().render(lang);
                let reason = (!e.reason.is_empty()).then(|| e.reason.clone());
                view! {
                    <li class="ui-list__row ui-flex-wrap">
                        <span class="ui-list__title t-mono">{e.repo.clone()}</span>
                        <Status tone=tone(e.state)>{state}</Status>
                        {shown.map(|b| view! { <span class="t-footnote t-secondary t-mono">{b}</span> })}
                        {pull
                            .map(|p| {
                                let label = t!(AgentReposPullRequestNumber, number = p.number);
                                if p.url.is_empty() {
                                    view! { <span class="t-footnote">{label}</span> }.into_any()
                                } else {
                                    view! {
                                        <a class="ui-link" href=p.url target="_blank" rel="noopener noreferrer">
                                            {label}
                                        </a>
                                    }
                                        .into_any()
                                }
                            })}
                        {reason.map(|m| view! { <p class="t-footnote ck-problem" role="status">{m}</p> })}
                    </li>
                }
            })
            .collect::<Vec<_>>();
        Some(view! {
            <div class="ui-list__section" role="region" aria-label=t!(AgentReposTitle)>
                <p class="ui-list__header">{t!(AgentReposTitle)}</p>
                <ul class="ui-list">{rows}</ul>
            </div>
        })
    }
}
