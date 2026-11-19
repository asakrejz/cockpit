//! Watched repos (optional): Cockpit finds the pull requests you are involved in across every repo
//! you can see. Repos picked here are loaded completely – all their open pull requests.

use cockpit_core::api::{ConfigChange, GitHubInput, GitHubRepoInfo, GitHubRepoList};
use cockpit_core::setup::{repo_matches, repo_summary, repos_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Message, config_save, error_text};
use super::repo_picker::static_text;
use crate::server;

/// At most this many entries are drawn; the search narrows the rest down.
const SHOWN: usize = 100;

#[component]
pub fn WatchedRepos(ctx: Context, opened: RwSignal<bool>) -> impl IntoView {
    let chosen: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let loaded = RwSignal::new(false);
    // Fill from the stored list once (it arrives after the page opens).
    Effect::new(move |_| {
        let stored = ctx.setup.with(|s| s.github_repos.clone());
        if !loaded.get_untracked() && !stored.is_empty() {
            loaded.set(true);
            chosen.set(stored);
        }
    });

    let list: RwSignal<Option<GitHubRepoList>> = RwSignal::new(None);
    let loading = RwSignal::new(false);
    let list_error: Feedback = RwSignal::new(None);
    let query = RwSignal::new(String::new());
    let load = move || {
        loading.set(true);
        list_error.set(None);
        spawn_local(async move {
            match server::fetch_github_repos().await {
                Ok(l) => list.set(Some(l)),
                Err(e) => {
                    list.set(None);
                    list_error.set(Some((false, error_text(&e))));
                }
            }
            loading.set(false);
        });
    };
    // Load when the section is opened for the first time and GitHub is connected.
    let started = RwSignal::new(false);
    Effect::new(move |_| {
        let connected = ctx
            .setup
            .with(|s| s.has_key("github") || s.has_key("github-oauth"));
        if opened.get() && connected && !started.get_untracked() {
            started.set(true);
            load();
        }
    });

    let toggle = move |info: GitHubRepoInfo| {
        chosen.update(|c| {
            if let Some(i) = c.iter().position(|r| *r == info.repo) {
                c.remove(i);
            } else {
                c.push(info.repo);
            }
        });
    };
    let filtered = Memo::new(move |_| {
        let q = query.get();
        list.with(|l| {
            l.as_ref()
                .map(|l| {
                    l.repos
                        .iter()
                        .filter(|r| repo_matches(&r.repo, &q))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    let visible =
        Memo::new(move |_| filtered.with(|f| f.iter().take(SHOWN).cloned().collect::<Vec<_>>()));
    let too_many = Memo::new(move |_| filtered.with(Vec::len) > SHOWN);

    let manual = RwSignal::new(String::new());
    let manual_feedback: Feedback = RwSignal::new(None);
    let add_manual = move |_| match repos_read(&manual.get_untracked()) {
        Ok(names) if names.is_empty() => {
            manual_feedback.set(Some((false, t!(SetupReposEnterRepo))));
        }
        Ok(names) => {
            chosen.update(|c| {
                for n in names {
                    if !c.contains(&n) {
                        c.push(n);
                    }
                }
            });
            manual.set(String::new());
            manual_feedback.set(None);
        }
        Err(m) => manual_feedback.set(Some((false, m.render(lang_untracked())))),
    };

    let feedback: Feedback = RwSignal::new(None);
    let save = move |_| {
        let repos = chosen.get_untracked();
        let none = repos.is_empty();
        config_save(
            ctx,
            ConfigChange {
                github: Some(GitHubInput { repos }),
                ..Default::default()
            },
            feedback,
            if none {
                t!(SetupReposWatchNoneLeft)
            } else {
                t!(SetupReposWatchSaved)
            },
            || {},
        );
    };

    view! {
        <VStack spacing="12px">
            <p class="t-subheadline t-secondary">{t!(SetupReposWatchIntro)}</p>
            <SearchField value=query placeholder=static_text(t!(SetupReposSearch)) />
            <HStack spacing="12px">
                <Button on_click=move |_| load() disabled=Signal::derive(move || loading.get())>
                    {move || {
                        if loading.get() {
                            t!(SetupReposLoading)
                        } else if list.with(Option::is_some) {
                            t!(SetupReposReload)
                        } else {
                            t!(SetupReposLoad)
                        }
                    }}
                </Button>
            </HStack>
            <Message feedback=list_error />
            {move || {
                list.with(|l| l.as_ref().map(|l| (l.repos.len(), l.truncated)))
                    .map(|(found, truncated)| {
                        view! {
                            <p class="t-footnote t-secondary" role="status">
                                {repo_summary(found, filtered.with(Vec::len), chosen.with(Vec::len), truncated).render(lang())}
                            </p>
                        }
                    })
            }}
            <ul class="ck-repo-list" aria-label=move || t!(SetupReposListLabel)>
                <For each=move || visible.get() key=|r| r.repo.clone() let:info>
                    {
                        let name = info.repo.clone();
                        let pick = info.clone();
                        view! {
                            <li>
                                <label class="ck-repo-item">
                                    <input
                                        type="checkbox"
                                        prop:checked={
                                            let name = name.clone();
                                            move || chosen.with(|c| c.contains(&name))
                                        }
                                        on:change=move |_| toggle(pick.clone())
                                    />
                                    <span class="ck-repo-name">{name.clone()}</span>
                                    <span class="t-footnote t-secondary">
                                        {if info.private { t!(SetupReposPrivate) } else { t!(SetupReposPublic) }}
                                        {info.archived.then(|| format!(" · {}", t!(SetupReposArchived)))}
                                    </span>
                                </label>
                            </li>
                        }
                    }
                </For>
            </ul>
            <Show when=move || too_many.get()>
                <p class="t-footnote t-secondary">{move || t!(SetupReposTooMany, max = SHOWN)}</p>
            </Show>
            <TextField
                label=t!(SetupReposManualLabel)
                value=manual
                placeholder="firma/app"
                hint=t!(SetupReposManualHint)
                autocomplete="off"
            />
            <HStack spacing="12px">
                <Button on_click=add_manual>{t!(SetupReposAdd)}</Button>
            </HStack>
            <Message feedback=manual_feedback />
            <span class="t-headline">{t!(SetupReposWatchChosen)}</span>
            <Show when=move || chosen.with(Vec::is_empty)>
                <p class="t-footnote t-secondary">{t!(SetupReposWatchNoneChosen)}</p>
            </Show>
            <ul class="ui-checklist">
                <For each=move || chosen.get() key=|r| r.clone() let:name>
                    <li>
                        <span class="ck-repo-name">{name.clone()}</span>
                        " "
                        <Button
                            variant=ButtonStyle::Borderless
                            on_click={
                                let name = name.clone();
                                move |_| chosen.update(|c| c.retain(|r| *r != name))
                            }
                        >
                            {t!(SetupReposRemove)}
                        </Button>
                    </li>
                </For>
            </ul>
            <HStack spacing="12px">
                <Button variant=ButtonStyle::GlassProminent on_click=save>
                    {t!(SetupReposWatchSave)}
                </Button>
            </HStack>
            <Message feedback=feedback />
        </VStack>
    }
}
