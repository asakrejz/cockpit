//! Step „GitHub Projects“: where the tickets come from, and which projects are read. The list comes
//! from `GET /api/github/projects` (read only); nothing is written to GitHub here.

use cockpit_core::api::{ConfigChange, GitHubProjectsInput, ProjectsResponse};
use cockpit_core::{ProjectOrigin, ProjectRef, Right, TicketsFrom, TokenKind};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Intro, Message, Section, config_save, error_text};
use crate::server;

fn from_label(from: TicketsFrom) -> String {
    match from {
        TicketsFrom::Jira => t!(SetupProjectsFromJira),
        TicketsFrom::GitHubProjects => t!(SetupProjectsFromProjects),
        TicketsFrom::Both => t!(SetupProjectsFromBoth),
    }
}

fn kind_label(kind: TokenKind) -> String {
    match kind {
        TokenKind::Classic => t!(SetupProjectsKindClassic),
        TokenKind::FineGrained => t!(SetupProjectsKindFineGrained),
        TokenKind::App => t!(SetupProjectsKindApp),
        TokenKind::Unknown => t!(SetupProjectsKindUnknown),
    }
}

/// The choice „Tickets from: Jira / GitHub Projects / Both“. Saved at once.
#[component]
pub fn TicketsFromPicker(ctx: Context) -> impl IntoView {
    let feedback: Feedback = RwSignal::new(None);
    let current = Signal::derive(move || ctx.setup.with(|s| s.github_projects.tickets_from));
    view! {
        <VStack spacing="8px">
            <div class="ui-segmented" role="group" aria-label=move || t!(SetupProjectsFromLabel) style="align-self: stretch">
                {TicketsFrom::ALL
                    .into_iter()
                    .map(|from| {
                        view! {
                            <button
                                type="button"
                                style="flex: 1 1 0; padding-inline: 8px"
                                aria-pressed=move || (current.get() == from).to_string()
                                on:click=move |_| {
                                    if current.get_untracked() == from {
                                        return;
                                    }
                                    config_save(
                                        ctx,
                                        ConfigChange {
                                            github_projects: Some(GitHubProjectsInput {
                                                tickets_from: Some(from),
                                                ..Default::default()
                                            }),
                                            ..Default::default()
                                        },
                                        feedback,
                                        t!(SetupProjectsFromSaved),
                                        || {},
                                    );
                                }
                            >
                                {move || from_label(from)}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
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
            <Section title=t!(SetupProjectsFromLabel)>
                <TicketsFromPicker ctx=ctx />
            </Section>
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
