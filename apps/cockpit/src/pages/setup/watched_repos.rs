//! Watched repos (optional): Cockpit finds the pull requests you are involved in across every repo
//! you can see. Repos picked here are loaded completely – all their open pull requests.

use cockpit_core::api::{ConfigChange, GitHubInput, GitHubRepoList};
use cockpit_core::setup::{repo_summary, repos_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Message, config_save, error_text};
use crate::repo_list::{RepoEntry, RepoList, RepoSearch};
use crate::server;

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

    let toggle = move |name: String| {
        chosen.update(|c| {
            if let Some(i) = c.iter().position(|r| *r == name) {
                c.remove(i);
            } else {
                c.push(name);
            }
        });
    };
    let search = RepoSearch::new(Memo::new(move |_| {
        list.with(|l| {
            l.as_ref()
                .map(|l| {
                    l.repos
                        .iter()
                        .map(|r| RepoEntry::from_github(r, false))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    }));

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
        <VStack class="ui-gap-12">
            <p class="t-subheadline t-secondary">{t!(SetupReposWatchIntro)}</p>
            <RepoList
                search=search
                label=Signal::derive(move || t!(SetupReposListLabel))
                selected=chosen
                on_toggle=Callback::new(toggle)
            >
                <HStack class="ui-gap-12">
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
                                    {repo_summary(found, search.matching(), chosen.with(Vec::len), truncated).render(lang())}
                                </p>
                            }
                        })
                }}
            </RepoList>
            <TextField
                label=t!(SetupReposManualLabel)
                value=manual
                placeholder=t!(SetupReposManualPlaceholder)
                hint=t!(SetupReposManualHint)
                autocomplete="off"
            />
            <HStack class="ui-gap-12">
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
            <HStack class="ui-gap-12">
                <Button variant=ButtonStyle::GlassProminent on_click=save>
                    {t!(SetupReposWatchSave)}
                </Button>
            </HStack>
            <Message feedback=feedback />
        </VStack>
    }
}
