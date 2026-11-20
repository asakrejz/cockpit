//! Settings per repo for the agent (optional): base branch, branch pattern, language of the pull
//! request text. Without an entry the agent uses the
//! default branch of the repo; the first start in a repo stores an entry here by itself. A repo is
//! chosen for each ticket when the agent starts, not here. The list to choose from comes from
//! GitHub (read-only).

use cockpit_core::api::{ConfigChange, GitHubRepoInfo, GitHubRepoList};
use cockpit_core::setup::{agent_repos_read, repo_matches, repo_summary, repos_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use cockpit_core::branch::DEFAULT_PATTERN;

use super::branch_pattern::BranchPatternField;
use super::fields::{Context, Feedback, Message, config_save, error_text};
use crate::server;

/// At most this many entries are drawn; the search narrows the rest down.
const SHOWN: usize = 100;

/// `SearchField` takes a `&'static str` placeholder. A translated text is leaked once per
/// distinct value (one per language), so the memory stays bounded.
pub(crate) fn static_text(text: String) -> &'static str {
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
    pattern: RwSignal<String>,
    /// Language code of the pull request text (`en` or `de`).
    pr_language: RwSignal<&'static str>,
    /// Does the sync load all open pull requests of this repo? Kept as stored; a repo added here
    /// is not watched (that is the list "Repos beobachten").
    watch: bool,
}

const PR_LANGUAGES: &[(&str, &str)] = &[
    (Lang::En.code(), Lang::En.name()),
    (Lang::De.code(), Lang::De.name()),
];

#[component]
pub fn RepoPicker(ctx: Context, opened: RwSignal<bool>) -> impl IntoView {
    let next_id = StoredValue::new(0usize);
    // The signals of a row belong to the page, not to the effect or click that creates the row:
    // those owners are cleaned up again and would take the signals with them.
    let owner = StoredValue::new(Owner::current());
    let new_row = move |repo: &str, base: &str, pattern: &str, watch: bool| {
        let id = next_id.get_value();
        next_id.set_value(id + 1);
        let make = || Row {
            id,
            repo: RwSignal::new(repo.into()),
            base: RwSignal::new(base.into()),
            pattern: RwSignal::new(pattern.into()),
            pr_language: RwSignal::new(Lang::En.code()),
            watch,
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
                    let row = new_row(&r.repo, &r.base_branch, pattern, r.watch);
                    row.pr_language.set(r.pr_language.code());
                    row
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
        if opened.get() && connected && !started.get_untracked() {
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
            let r = new_row(&name, base, "", false);
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
                    let r = new_row(&n, "main", "", false);
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
        let list: Vec<(String, String, String)> = rows
            .get_untracked()
            .iter()
            .map(|r| {
                (
                    r.repo.get_untracked(),
                    r.base.get_untracked(),
                    r.pattern.get_untracked(),
                )
            })
            .collect();
        match agent_repos_read(&list, &default.get_untracked()) {
            Ok(mut input) => {
                // The language of the pull request text belongs to the row of the repo.
                for r in &mut input.repos {
                    let code = rows.with_untracked(|rows| {
                        rows.iter()
                            .find(|row| row.repo.get_untracked().trim() == r.repo)
                            .map(|row| row.pr_language.get_untracked())
                    });
                    r.pr_language = code.and_then(Lang::from_code).unwrap_or_default();
                    r.watch = rows.with_untracked(|rows| {
                        rows.iter()
                            .find(|row| row.repo.get_untracked().trim() == r.repo)
                            .is_some_and(|row| row.watch)
                    });
                }
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
        <VStack spacing="12px">
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
                        <BranchPatternField repo=row.repo pattern=row.pattern />
                        <VStack spacing="6px">
                            <span class="t-subheadline">{t!(SetupReposPrLanguage)}</span>
                            <SegmentedPicker
                                options=PR_LANGUAGES
                                value=row.pr_language
                                on_change=Callback::new(move |code: &'static str| row.pr_language.set(code))
                                aria_label=t!(SetupReposPrLanguage)
                            />
                            <span class="t-footnote t-secondary">{t!(SetupReposPrLanguageHint)}</span>
                        </VStack>
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
        </VStack>
    }
}
