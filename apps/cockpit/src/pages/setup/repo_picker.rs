//! The repos Cockpit works with: ONE list. A repo chosen here is a source of the comparison
//! (its pull requests) and a choice for the agent. Per repo: base branch and, optionally, the
//! Jira projects whose tickets go there. The list to choose from comes from GitHub (read-only).

use cockpit_core::api::{ConfigChange, GitHubRepoInfo, GitHubRepoList};
use cockpit_core::setup::{agent_repos_read, repo_matches, repo_summary, repos_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use cockpit_core::branch::DEFAULT_PATTERN;

use super::branch_pattern::BranchPatternField;
use super::fields::{Context, Feedback, Message, Section, config_save, error_text};
use crate::server;

/// At most this many entries are drawn; the search narrows the rest down.
const SHOWN: usize = 100;

/// `SearchField` takes a `&'static str` placeholder. A translated text is leaked once per
/// distinct value (one per language), so the memory stays bounded.
fn static_text(text: String) -> &'static str {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static CACHE: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
    }
    CACHE.with(|c| {
        *c.borrow_mut()
            .entry(text)
            .or_insert_with_key(|k| Box::leak(k.clone().into_boxed_str()))
    })
}

/// One chosen repo. The signals live as long as the page.
#[derive(Clone, Copy)]
struct Row {
    id: usize,
    repo: RwSignal<String>,
    base: RwSignal<String>,
    projects: RwSignal<String>,
    pattern: RwSignal<String>,
}

#[component]
pub fn RepoPicker(ctx: Context) -> impl IntoView {
    let next_id = StoredValue::new(0usize);
    // The signals of a row belong to the page, not to the effect or click that creates the row:
    // those owners are cleaned up again and would take the signals with them.
    let owner = StoredValue::new(Owner::current());
    let new_row = move |repo: &str, base: &str, projects: &str, pattern: &str| {
        let id = next_id.get_value();
        next_id.set_value(id + 1);
        let make = || Row {
            id,
            repo: RwSignal::new(repo.into()),
            base: RwSignal::new(base.into()),
            projects: RwSignal::new(projects.into()),
            pattern: RwSignal::new(pattern.into()),
        };
        match owner.get_value() {
            Some(o) => o.with(make),
            None => make(),
        }
    };
    let rows: RwSignal<Vec<Row>> = RwSignal::new(Vec::new());
    let default = RwSignal::new(String::new());
    let loaded = RwSignal::new(false);
    // Fill from the stored values once (they arrive after the page opens).
    Effect::new(move |_| {
        let stored = ctx.setup.with(|s| s.agent_repos.clone());
        if loaded.get_untracked() || (stored.repos.is_empty() && stored.default.is_none()) {
            return;
        }
        loaded.set(true);
        rows.set(
            stored
                .repos
                .iter()
                .map(|r| {
                    let pattern = if r.branch_pattern == DEFAULT_PATTERN {
                        ""
                    } else {
                        &r.branch_pattern
                    };
                    new_row(&r.repo, &r.base_branch, &r.projects.join(", "), pattern)
                })
                .collect(),
        );
        default.set(stored.default.unwrap_or_default());
    });

    // The list to choose from.
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
    // Load once as soon as a key exists; later the button does it.
    let started = RwSignal::new(false);
    Effect::new(move |_| {
        let connected = ctx
            .setup
            .with(|s| s.has_key("github") || s.has_key("github-oauth"));
        if connected && !started.get_untracked() {
            started.set(true);
            load();
        }
    });

    let chosen = Memo::new(move |_| {
        rows.get()
            .iter()
            .map(|r| r.repo.get().trim().to_string())
            .collect::<Vec<_>>()
    });
    let toggle = move |info: GitHubRepoInfo| {
        let name = info.repo.clone();
        if chosen.get_untracked().contains(&name) {
            rows.update(|l| l.retain(|r| r.repo.get_untracked().trim() != name));
            if default.get_untracked() == name {
                default.set(String::new());
            }
        } else {
            let base = if info.default_branch.is_empty() {
                "main"
            } else {
                &info.default_branch
            };
            let r = new_row(&name, base, "", "");
            rows.update(|l| l.push(r));
        }
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

    // A repo typed by hand (not in the list, e.g. when the list could not be loaded).
    let manual = RwSignal::new(String::new());
    let manual_feedback: Feedback = RwSignal::new(None);
    let add_manual = move |_| match repos_read(&manual.get_untracked()) {
        Ok(names) if names.is_empty() => {
            manual_feedback.set(Some((false, t!(SetupReposEnterRepo))));
        }
        Ok(names) => {
            for n in names {
                if !chosen.get_untracked().contains(&n) {
                    let r = new_row(&n, "main", "", "");
                    rows.update(|l| l.push(r));
                }
            }
            manual.set(String::new());
            manual_feedback.set(None);
        }
        Err(m) => manual_feedback.set(Some((false, m.render(lang_untracked())))),
    };

    let feedback: Feedback = RwSignal::new(None);
    let save = move |_| {
        let list: Vec<(String, String, String, String)> = rows
            .get_untracked()
            .iter()
            .map(|r| {
                (
                    r.repo.get_untracked(),
                    r.base.get_untracked(),
                    r.projects.get_untracked(),
                    r.pattern.get_untracked(),
                )
            })
            .collect();
        match agent_repos_read(&list, &default.get_untracked()) {
            Ok(input) => {
                let n = input.repos.len();
                config_save(
                    ctx,
                    ConfigChange {
                        agent_repos: Some(input),
                        ..Default::default()
                    },
                    feedback,
                    if n == 0 {
                        t!(SetupReposNoneLeft)
                    } else {
                        t!(SetupReposSaved)
                    },
                    || {},
                );
            }
            Err(m) => feedback.set(Some((false, m.render(lang_untracked())))),
        }
    };

    view! {
        <Section title=t!(SetupReposTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupReposIntro)}
            </p>
            <SearchField value=query placeholder=static_text(t!(SetupReposSearch)) />
            <HStack spacing="12px">
                <Button
                    on_click=move |_| load()
                    disabled=Signal::derive(move || loading.get())
                >
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
                list.with(|l| l.as_ref().map(|l| (l.repos.len(), l.truncated, l.via.clone())))
                    .map(|(found, truncated, via)| {
                        view! {
                            <p class="t-footnote t-secondary" role="status">
                                {repo_summary(found, filtered.with(Vec::len), chosen.with(Vec::len), truncated).render(lang())}
                                {(via == "app").then(|| format!(" {}", t!(SetupReposViaApp)))}
                            </p>
                        }
                    })
            }}
            <ul class="ck-repo-list" aria-label=move || t!(SetupReposListLabel)>
                <For
                    each=move || visible.get()
                    key=|r| r.repo.clone()
                    let:info
                >
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
                                        {(info.can_push == Some(false)).then(|| format!(" · {}", t!(SetupReposReadOnly)))}
                                    </span>
                                </label>
                            </li>
                        }
                    }
                </For>
            </ul>
            <Show when=move || too_many.get()>
                <p class="t-footnote t-secondary">
                    {move || t!(SetupReposTooMany, max = SHOWN)}
                </p>
            </Show>
            <div class="ck-agent-repo">
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
            </div>
            <span class="t-headline">{t!(SetupReposChosen)}</span>
            <Show when=move || rows.with(Vec::is_empty)>
                <p class="t-footnote t-secondary">{t!(SetupReposNoneChosen)}</p>
            </Show>
            <VStack spacing="12px">
                <For each=move || rows.get() key=|r| r.id let:row>
                    <div class="ck-agent-repo">
                        <span class="ck-repo-name">{move || row.repo.get()}</span>
                        <TextField label=t!(SetupReposBaseBranch) value=row.base placeholder="main" autocomplete="off" />
                        <TextField
                            label=t!(SetupReposProjects)
                            value=row.projects
                            placeholder="WEB, SHOP"
                            hint=t!(SetupReposProjectsHint)
                            autocomplete="off"
                        />
                        <BranchPatternField repo=row.repo pattern=row.pattern />
                        <HStack spacing="12px">
                            <label style="display: inline-flex; gap: 6px; align-items: center; cursor: pointer">
                                <input
                                    type="radio"
                                    name="agent-default-repo"
                                    prop:checked=move || {
                                        let r = row.repo.get();
                                        !r.trim().is_empty() && default.get() == r.trim()
                                    }
                                    on:change=move |_| default.set(row.repo.get_untracked().trim().to_string())
                                />
                                {t!(SetupReposDefaultRepo)}
                            </label>
                            <Button
                                variant=ButtonStyle::Borderless
                                on_click=move |_| {
                                    let name = row.repo.get_untracked().trim().to_string();
                                    if default.get_untracked() == name {
                                        default.set(String::new());
                                    }
                                    rows.update(|l| l.retain(|r| r.id != row.id));
                                }
                            >
                                {t!(SetupReposRemove)}
                            </Button>
                        </HStack>
                    </div>
                </For>
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=save>
                        {t!(SetupReposSave)}
                    </Button>
                </HStack>
                <Message feedback=feedback />
            </VStack>
        </Section>
    }
}
