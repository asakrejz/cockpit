//! The repo of the run, chosen when the agent starts: a searchable list of every repo GitHub
//! shows, with the suggestions for this ticket first and a short reason for each.

use cockpit_core::model::repo_name_valid;
use cockpit_core::repos::suggest::{SuggestReason, picker_rows};
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;
use crate::pages::setup::static_text;

/// At most this many lines are drawn; the search narrows the rest down.
const SHOWN: usize = 100;

/// The short sentence for a reason.
fn reason_text(reason: &SuggestReason) -> String {
    match reason {
        SuggestReason::PreviousRun { project, .. } => {
            t!(AgentJobReasonPreviousRun, project = project.clone())
        }
        SuggestReason::Issue => t!(AgentJobReasonIssue),
        SuggestReason::LinkedPr => t!(AgentJobReasonLinkedPr),
        SuggestReason::Mentioned => t!(AgentJobReasonMentioned),
        SuggestReason::Rule { project } => t!(AgentJobReasonRule, project = project.clone()),
        SuggestReason::Default => t!(AgentJobReasonDefault),
    }
}

pub(super) fn repo_choice(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx {
        repo_touched,
        project,
        repo_list,
        repo_list_error,
        repo_loading,
        suggestions,
        locked,
        ..
    } = ctx;
    let repo = ctx.form.repo;
    let others = ctx.form.others;
    let agent_repos = ctx.form.agent_repos;
    let query = RwSignal::new(String::new());

    let all = Memo::new(move |_| {
        repo_list.with(|l| {
            l.as_ref()
                .map(|l| l.repos.iter().map(|r| r.repo.clone()).collect::<Vec<_>>())
                .unwrap_or_default()
        })
    });
    // Without GitHub's list the configured repos are still there to choose from.
    let known = Memo::new(move |_| {
        let mut list = all.get();
        if list.is_empty() {
            list = agent_repos.with(|a| a.names());
        }
        list
    });
    let rows = Memo::new(move |_| {
        let q = query.get();
        suggestions.with(|s| known.with(|k| picker_rows(s, k, &q, SHOWN + 1)))
    });
    let too_many = Memo::new(move |_| rows.with(Vec::len) > SHOWN);
    let visible =
        Memo::new(move |_| rows.with(|r| r.iter().take(SHOWN).cloned().collect::<Vec<_>>()));
    // A name typed in the search that is a valid repo but not in the list.
    let typed = Memo::new(move |_| {
        let q = query.get().trim().to_string();
        (repo_name_valid(&q)
            && !rows.with(|r| r.iter().any(|row| row.repo.eq_ignore_ascii_case(&q))))
        .then_some(q)
    });
    // All chosen repos, the first one is the primary one.
    let selected = Memo::new(move |_| {
        let first = repo.get();
        if first.is_empty() {
            return Vec::new();
        }
        std::iter::once(first)
            .chain(others.get())
            .collect::<Vec<_>>()
    });
    // Adds the repo, or takes it away again if it is already chosen. At least one stays chosen
    // only in the sense that the start needs one; removing the last one leaves the choice empty.
    let pick = move |name: String| {
        repo_touched.set(true);
        let mut all = selected.get_untracked();
        if let Some(i) = all.iter().position(|r| *r == name) {
            all.remove(i);
        } else if all.len() < cockpit_core::MAX_REPOS_PER_RUN {
            all.push(name);
        }
        let mut it = all.into_iter();
        repo.set(it.next().unwrap_or_default());
        others.set(it.collect());
    };
    // The repo has no entry in the settings yet.
    let first_use = Signal::derive(move || {
        selected.with(|s| s.iter().any(|r| agent_repos.with(|a| a.find(r).is_none())))
    });

    view! {
        <div class="ck-repo-choice">
            <span class="t-subheadline">{t!(AgentJobRepo)}</span>
            <Show when=move || !repo.get().is_empty()>
                <p role="status" class="ui-hstack" style="--spacing: 6px; flex-wrap: wrap">
                    <For each=move || selected.get() key=|r| r.clone() let:name>
                        {
                            let remove = name.clone();
                            view! {
                                <Chip>{name}</Chip>
                                <Show when=move || !locked.get()>
                                    <Button
                                        size=ControlSize::Small
                                        variant=ButtonStyle::Borderless
                                        on_click={
                                            let remove = remove.clone();
                                            move |_| pick(remove.clone())
                                        }
                                    >
                                        {t!(AgentJobRepoRemove)}
                                    </Button>
                                </Show>
                            }
                        }
                    </For>
                </p>
                <Show when=move || { selected.with(Vec::len) > 1 }>
                    <p class="t-footnote t-secondary">{t!(AgentJobReposHint)}</p>
                </Show>
                <Show when=move || first_use.get() && !locked.get()>
                    <p class="t-footnote t-secondary">{t!(AgentJobRepoNewHint)}</p>
                </Show>
            </Show>
            <Show when=move || !locked.get()>
                <SearchField value=query placeholder=static_text(t!(AgentJobRepoSearch)) />
                <Show when=move || repo_loading.get()>
                    <p class="t-footnote t-secondary" role="status">{t!(AgentJobRepoLoading)}</p>
                </Show>
                {move || {
                    repo_list_error
                        .get()
                        .map(|e| {
                            view! {
                                <p class="t-footnote t-secondary" role="status">
                                    {t!(AgentJobRepoNotLoaded, error = e)}
                                </p>
                            }
                        })
                }}
                <ul class="ck-repo-list" aria-label=move || t!(AgentJobRepoListLabel)>
                    <For each=move || visible.get() key=|r| r.repo.clone() let:row>
                        {
                            let name = row.repo.clone();
                            let chosen = {
                                let name = name.clone();
                                move || selected.with(|s| s.contains(&name))
                            };
                            let on_pick = {
                                let name = name.clone();
                                move |_| pick(name.clone())
                            };
                            view! {
                                <li>
                                    <label class="ck-repo-item">
                                        <input
                                            type="checkbox"
                                            name="agent-repo"
                                            prop:checked=chosen
                                            on:change=on_pick
                                        />
                                        <span class="ck-repo-name">{name}</span>
                                        <span class="t-footnote t-secondary">
                                            {row.reason.as_ref().map(reason_text)}
                                        </span>
                                    </label>
                                </li>
                            }
                        }
                    </For>
                </ul>
                <Show when=move || too_many.get()>
                    <p class="t-footnote t-secondary">{move || t!(AgentJobRepoTooMany, max = SHOWN)}</p>
                </Show>
                <Show when=move || {
                    !repo_loading.get() && rows.with(Vec::is_empty) && typed.get().is_none()
                }>
                    <p class="t-footnote t-secondary">{t!(AgentJobRepoNoneFound)}</p>
                </Show>
                {move || {
                    typed
                        .get()
                        .map(|name| {
                            let label = t!(AgentJobRepoManual, repo = name.clone());
                            view! {
                                <HStack spacing="12px">
                                    <Button on_click=move |_| pick(name.clone())>{label}</Button>
                                </HStack>
                            }
                        })
                }}
                <Show when=move || repo.get().is_empty()>
                    <p class="t-footnote t-secondary" role="status">
                        {move || match project.get() {
                            Some(p) => t!(AgentJobNoRepoProject, project = p),
                            None => t!(AgentJobNoRepo),
                        }}
                    </p>
                </Show>
            </Show>
        </div>
    }
}
