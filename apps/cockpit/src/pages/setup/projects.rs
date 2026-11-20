//! Step „GitHub Projects“: which projects the tickets are read from (shown when the tickets come
//! from GitHub Projects; the choice of the source is the step „Tickets“). The list comes
//! from `GET /api/github/projects` (read only); nothing is written to GitHub here.

use cockpit_core::api::{ConfigChange, GitHubProjectsInput, ProjectsResponse};
use cockpit_core::{ProjectOrigin, ProjectRef, Right, TokenKind};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Intro, Message, Section, config_save, error_text};
use crate::server;

fn kind_label(kind: TokenKind) -> String {
    match kind {
        TokenKind::Classic => t!(SetupProjectsKindClassic),
        TokenKind::FineGrained => t!(SetupProjectsKindFineGrained),
        TokenKind::App => t!(SetupProjectsKindApp),
        TokenKind::Unknown => t!(SetupProjectsKindUnknown),
    }
}

/// "Projekt anlegen" for one repo that has no linked project. Nothing is created before the click.
#[component]
fn CreateProject(
    ctx: Context,
    repo: String,
    owners: Vec<String>,
    created: Callback<cockpit_core::ProjectSummary>,
) -> impl IntoView {
    let repo = StoredValue::new(repo);
    // The owner of the repo if it is one of the choices, otherwise the first (the user).
    let default_owner = owners
        .iter()
        .find(|o| {
            repo.with_value(|r| {
                r.split('/')
                    .next()
                    .is_some_and(|x| x.eq_ignore_ascii_case(o))
            })
        })
        .or(owners.first())
        .cloned()
        .unwrap_or_default();
    let owner = RwSignal::new(default_owner);
    let title = RwSignal::new(format!(
        "{} board",
        repo.with_value(|r| r.split('/').nth(1).unwrap_or(r).to_string())
    ));
    let running = RwSignal::new(false);
    let feedback: Feedback = RwSignal::new(None);
    let options: Vec<(String, String)> = owners.iter().map(|o| (o.clone(), o.clone())).collect();
    let create = move |_| {
        running.set(true);
        feedback.set(None);
        let request = cockpit_core::api::CreateProjectRequest {
            owner: owner.get_untracked(),
            title: title.get_untracked(),
            repo: Some(repo.get_value()),
        };
        spawn_local(async move {
            match server::post_project_create(&request).await {
                Ok(done) => {
                    feedback.set(Some((
                        true,
                        t!(
                            SetupProjectsCreateDone,
                            title = done.project.title.clone(),
                            repo = repo.get_value()
                        ),
                    )));
                    created.run(done.project);
                    ctx.reload();
                }
                Err(e) => feedback.set(Some((
                    false,
                    t!(SetupProjectsCreateFailed, text = error_text(&e)),
                ))),
            }
            running.set(false);
        });
    };
    view! {
        <VStack spacing="8px">
            <p class="t-footnote t-secondary">{t!(SetupProjectsCreateIntro, repo = repo.get_value())}</p>
            <Select
                options=options
                value=owner
                label=t!(SetupProjectsCreateOwner)
                on_change=move |v: String| owner.set(v)
            />
            <TextField label=t!(SetupProjectsCreateName) value=title />
            <HStack spacing="12px">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=Signal::derive(move || {
                        running.get() || title.with(|t| t.trim().is_empty()) || owner.with(String::is_empty)
                    })
                    on_click=create
                >
                    {move || if running.get() { t!(SetupProjectsCreateRunning) } else { t!(SetupProjectsCreateButton) }}
                </Button>
            </HStack>
            <Message feedback=feedback />
        </VStack>
    }
}

#[component]
pub fn ProjectsStep(ctx: Context) -> impl IntoView {
    let connected = Signal::derive(move || {
        ctx.setup
            .with(|s| s.has_key("github") || s.has_key("github-oauth"))
    });
    let list: RwSignal<Option<ProjectsResponse>> = RwSignal::new(None);
    let loading = RwSignal::new(false);
    let list_error: Feedback = RwSignal::new(None);
    let load = move || {
        loading.set(true);
        list_error.set(None);
        spawn_local(async move {
            match server::fetch_github_projects().await {
                Ok(l) => list.set(Some(l)),
                Err(e) => {
                    list.set(None);
                    list_error.set(Some((false, error_text(&e))));
                }
            }
            loading.set(false);
        });
    };
    let started = RwSignal::new(false);
    Effect::new(move |_| {
        if connected.get() && !started.get_untracked() {
            started.set(true);
            load();
        }
    });

    // The chosen projects (`owner/number`), filled from the stored choice once.
    let chosen: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let status_field = RwSignal::new(String::new());
    let team_field = RwSignal::new(String::new());
    let filled = RwSignal::new(false);
    Effect::new(move |_| {
        if ctx.ready.get() && !filled.get_untracked() {
            filled.set(true);
            ctx.setup.with_untracked(|s| {
                chosen.set(s.github_projects.projects.clone());
                status_field.set(s.github_projects.status_field.clone().unwrap_or_default());
                team_field.set(s.github_projects.team_field.clone().unwrap_or_default());
            });
        }
    });
    let toggle = move |name: String| {
        chosen.update(|c| match c.iter().position(|p| *p == name) {
            Some(i) => {
                c.remove(i);
            }
            None => c.push(name),
        });
    };

    let manual = RwSignal::new(String::new());
    let manual_feedback: Feedback = RwSignal::new(None);
    let add_manual = move |_| match ProjectRef::parse(&manual.get_untracked()) {
        Some(r) => {
            let name = r.to_string();
            chosen.update(|c| {
                if !c.contains(&name) {
                    c.push(name);
                }
            });
            manual.set(String::new());
            manual_feedback.set(None);
        }
        None => manual_feedback.set(Some((false, t!(SetupProjectsManualInvalid)))),
    };

    let feedback: Feedback = RwSignal::new(None);
    let save = move |_| {
        config_save(
            ctx,
            ConfigChange {
                github_projects: Some(GitHubProjectsInput {
                    projects: Some(chosen.get_untracked()),
                    status_field: Some(status_field.get_untracked()),
                    team_field: Some(team_field.get_untracked()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            feedback,
            t!(SetupProjectsSaved),
            || {},
        );
    };

    view! {
        <VStack spacing="20px">
            <Intro title=t!(SetupProjectsTitle)>
                <p>{t!(SetupProjectsIntro)}</p>
            </Intro>
            <Show
                when=move || connected.get()
                fallback=move || {
                    view! {
                        <p class="ui-hint" role="status">
                            {t!(SetupProjectsNeedGithub)}
                            <super::fields::StepLink step=cockpit_core::setup::Step::GitHub />
                        </p>
                    }
                }
            >
                <Section title=t!(SetupProjectsListLabel)>
                    <HStack spacing="12px">
                        <Button on_click=move |_| load() disabled=Signal::derive(move || loading.get())>
                            {move || {
                                if loading.get() {
                                    t!(SetupProjectsLoading)
                                } else if list.with(Option::is_some) {
                                    t!(SetupProjectsReload)
                                } else {
                                    t!(SetupProjectsLoad)
                                }
                            }}
                        </Button>
                    </HStack>
                    <Message feedback=list_error />
                    {move || {
                        list.get()
                            .and_then(|l| l.problem)
                            .map(|m| {
                                view! {
                                    <p class="ui-hint" role="alert" style="color: var(--sys-color-red)">
                                        {t!(SetupProjectsProblem, text = m.render(lang()))}
                                    </p>
                                }
                            })
                    }}
                    <Show when=move || {
                        list.with(|l| l.as_ref().is_some_and(|l| l.projects.is_empty() && l.problem.is_none()))
                    }>
                        <p class="t-footnote t-secondary">{t!(SetupProjectsNoneFound)}</p>
                    </Show>
                    <ul class="ck-repo-list" aria-label=move || t!(SetupProjectsListLabel)>
                        <For
                            each=move || list.get().map(|l| l.projects).unwrap_or_default()
                            key=|p| p.id.clone()
                            let:p
                        >
                            {
                                let name = p.reference().to_string();
                                let pick = name.clone();
                                let origin = match p.origin {
                                    ProjectOrigin::User => t!(SetupProjectsOriginUser),
                                    ProjectOrigin::Org => t!(SetupProjectsOriginOrg),
                                    ProjectOrigin::Repo => t!(SetupProjectsOriginRepo),
                                };
                                let linked = (!p.repos.is_empty())
                                    .then(|| t!(SetupProjectsLinkedTo, repos = p.repos.join(", ")));
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
                                            <span class="ck-repo-name">{p.title.clone()}</span>
                                            <span class="t-footnote t-secondary">
                                                {name.clone()} " · " {origin}
                                                {linked.map(|l| format!(" · {l}"))}
                                                {p.closed.then(|| format!(" · {}", t!(SetupProjectsClosed)))}
                                            </span>
                                        </label>
                                    </li>
                                }
                            }
                        </For>
                    </ul>
                    <TextField
                        label=t!(SetupProjectsManualLabel)
                        value=manual
                        placeholder="acme/3"
                        hint=t!(SetupProjectsManualHint)
                        autocomplete="off"
                    />
                    <HStack spacing="12px">
                        <Button on_click=add_manual>{t!(SetupProjectsAdd)}</Button>
                    </HStack>
                    <Message feedback=manual_feedback />
                    <Show when=move || chosen.with(Vec::is_empty)>
                        <p class="t-footnote t-secondary">{t!(SetupProjectsChosenNone)}</p>
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
                                    {t!(SetupProjectsRemove)}
                                </Button>
                            </li>
                        </For>
                    </ul>
                    <TextField
                        label=t!(SetupProjectsStatusField)
                        value=status_field
                        placeholder="Status"
                        hint=t!(SetupProjectsStatusHint)
                        autocomplete="off"
                    />
                    <TextField
                        label=t!(SetupProjectsTeamField)
                        value=team_field
                        placeholder="Team"
                        hint=t!(SetupProjectsTeamHint)
                        autocomplete="off"
                    />
                    <HStack spacing="12px">
                        <Button variant=ButtonStyle::GlassProminent on_click=save>
                            {t!(SetupProjectsSave)}
                        </Button>
                    </HStack>
                    <Message feedback=feedback />
                </Section>
                // A repo in use without a linked project gets the offer to create one.
                {move || {
                    let linked = list.get().map(|l| l.projects).unwrap_or_default();
                    let owners = list.get().map(|l| l.owners).unwrap_or_default();
                    let repos = ctx.setup.with(|s| s.github_repos.clone());
                    let loaded = list.with(|l| l.as_ref().is_some_and(|l| l.problem.is_none()));
                    (loaded && !owners.is_empty())
                        .then(|| {
                            repos
                                .into_iter()
                                .filter(|r| !linked.iter().any(|p| p.repos.contains(r)))
                                .map(|repo| {
                                    let owners = owners.clone();
                                    let added = Callback::new(move |p: cockpit_core::ProjectSummary| {
                                        let name = p.reference().to_string();
                                        chosen.update(|c| {
                                            if !c.contains(&name) {
                                                c.push(name);
                                            }
                                        });
                                        // The new project is chosen right away.
                                        config_save(
                                            ctx,
                                            ConfigChange {
                                                github_projects: Some(GitHubProjectsInput {
                                                    projects: Some(chosen.get_untracked()),
                                                    ..Default::default()
                                                }),
                                                ..Default::default()
                                            },
                                            RwSignal::new(None),
                                            String::new(),
                                            load,
                                        );
                                    });
                                    view! {
                                        <Section title=t!(SetupProjectsCreateTitle)>
                                            <CreateProject ctx=ctx repo=repo owners=owners created=added />
                                        </Section>
                                    }
                                })
                                .collect_view()
                        })
                }}
                <Section title=t!(SetupProjectsRightsTitle)>
                    {move || {
                        list.get()
                            .map(|l| {
                                let rights = l.rights;
                                let missing_read = rights
                                    .missing
                                    .iter()
                                    .any(|m| matches!(m.right, Right::ProjectsRead | Right::IssuesRead));
                                view! {
                                    <p class="t-subheadline">{kind_label(rights.kind)}</p>
                                    {rights
                                        .scopes
                                        .map(|s| {
                                            view! { <p class="t-footnote t-secondary t-mono">{s.join(", ")}</p> }
                                        })}
                                    {if rights.missing.is_empty() && rights.kind == TokenKind::Classic {
                                        view! { <p class="t-footnote t-secondary">{t!(SetupProjectsRightsReadOk)}</p> }
                                            .into_any()
                                    } else if rights.missing.is_empty() {
                                        view! { <p class="t-footnote t-secondary">{t!(SetupProjectsRightsUnknown)}</p> }
                                            .into_any()
                                    } else {
                                        view! {
                                            <p class="t-footnote" role=if missing_read { "alert" } else { "status" }>
                                                {t!(SetupProjectsRightsMissing)}
                                            </p>
                                            <ul class="ui-checklist">
                                                {rights
                                                    .missing
                                                    .into_iter()
                                                    .map(|m| view! { <li>{m.msg.render(lang())}</li> })
                                                    .collect_view()}
                                            </ul>
                                        }
                                            .into_any()
                                    }}
                                }
                            })
                    }}
                    <p class="t-footnote t-secondary">{t!(SetupProjectsRightsTable)}</p>
                </Section>
            </Show>
        </VStack>
    }
}
