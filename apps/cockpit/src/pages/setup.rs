//! Setup: a guided assistant in six steps – container, Jira, GitHub, model access,
//! optional live update (webhooks) and a summary. Every step can be skipped and caught up on later.
//!
//! Keys (client secrets, tokens, the key of the model endpoint) go into password fields and go once
//! to the local server, which stores them in the .env file. The browser stores them nowhere
//! (no `localStorage`, no address), the server never sends them back. The rules for this
//! are in `cockpit_core::setup`, writing `config.toml` is in the server.

mod auth_method;
mod branch_pattern;
mod build_log;
mod container;
mod done;
mod fields;
mod github;
mod jira;
mod live;
mod model;
mod optional;
mod projects;
mod repo_picker;
mod source;
pub(crate) use fields::error_text;
mod watched_repos;

use cockpit_core::setup::{Snapshot, Step, first_open_step, summary};
use feature_ui::*;
use leptos::prelude::*;

use crate::route::Route;
use crate::store::{navigate, use_store};
use container::ContainerStep;
use done::DoneStep;
use fields::Context;
use github::GitHubStep;
use jira::JiraStep;
use live::LiveStep;
use model::ModelStep;
use projects::ProjectsStep;
use source::SourceStep;

/// The progress display: one item per step in the order of [`Step::ALL`]. All steps before the
/// current one count as done.
#[component]
fn StepProgress(
    #[prop(into)] current: Signal<usize>,
    #[prop(into)] from: Signal<cockpit_core::TicketsFrom>,
) -> impl IntoView {
    view! {
        <ol class="ui-steps" aria-label=move || t!(SetupProgress)>
            {Step::ALL
                .iter()
                .enumerate()
                .map(|(i, step)| {
                    let step = *step;
                    view! {
                        <li
                            class="ui-steps__item"
                            style:display=move || (!step.shown(from.get())).then_some("none")
                            aria-current=move || (current.get() == i).then_some("step")
                            data-done=move || (current.get() > i).then_some("")
                        >
                            <span class="ui-steps__no">{i + 1}</span>
                            {move || fields::step_label(step)}
                        </li>
                    }
                })
                .collect_view()}
        </ol>
    }
}

#[component]
pub fn SetupPage() -> impl IntoView {
    let store = use_store();
    // Every visit asks anew: a lot can have happened between two visits.
    let ctx = Context::create(store);
    let step = RwSignal::new(Step::Container);
    let index = Signal::derive(move || step.get().index());
    // The step is part of the address (`#/setup/jira`), so it survives a reload of the page and
    // the back button works. Without a step in the address the assistant opens at the first step
    // that is not done yet (see below), once.
    let route_step = Signal::derive(move || match store.route.get() {
        Route::SetupStep(s) => Some(s),
        _ => None,
    });
    let ready = RwSignal::new(route_step.get_untracked().is_some());
    if let Some(s) = route_step.get_untracked() {
        step.set(s);
    }
    // Address -> step (back button, links from other pages).
    Effect::new(move |_| {
        if let Some(s) = route_step.get() {
            if step.get_untracked() != s {
                step.set(s);
            }
            ready.set(true);
        }
    });
    // Step -> address.
    Effect::new(move |_| {
        let s = step.get();
        if ready.get() && route_step.get_untracked() != Some(s) {
            navigate(&Route::SetupStep(s));
        }
    });
    // Without a step in the address: wait for the state of the server, then pick the first open step.
    Effect::new(move |_| {
        if ready.get_untracked() || !ctx.ready.get() {
            return;
        }
        let Some(health) = store.health.get() else {
            return;
        };
        let lines = summary(&Snapshot {
            setup: ctx.setup.get_untracked(),
            jira: health.sign_in.jira.clone(),
            github: health.sign_in.github.clone(),
            endpoint: health.endpoint.clone(),
            container: health.container.clone(),
            agent: health.agent.clone(),
            check: None,
            github_access: None,
        });
        // The address effect above takes the step over from here.
        store.replace_route(&Route::SetupStep(first_open_step(&lines)));
    });

    let from = Signal::derive(move || ctx.setup.with(|s| s.github_projects.tickets_from));
    let next_step = move |_| {
        if let Some(n) = step.get_untracked().next_for(from.get_untracked()) {
            step.set(n);
        }
    };
    let back = move |_| {
        if let Some(v) = step.get_untracked().previous_for(from.get_untracked()) {
            step.set(v);
        }
    };
    let jump = Callback::new(move |s: Step| step.set(s));

    view! {
        <div class="ck-setup">
            <Container readable=true>
                <VStack spacing="28px">
                    <VStack spacing="10px">
                        <HStack spacing="10px">
                            <span style="display: inline-flex; color: var(--sys-color-tint)">
                                <Icon path=icons::BRAND size=26 />
                            </span>
                            <span class="t-title3 t-bold">"Cockpit"</span>
                            <Badge variant=BadgeVariant::Neutral>{t!(SetupBadgeLocal)}</Badge>
                        </HStack>
                        <crate::language::LanguageSwitch />
                        <h1 class="t-display">{t!(SetupTitle)}</h1>
                        <p class="t-title3 t-secondary">{t!(SetupIntro)}</p>
                    </VStack>

                    <StepProgress current=index from=from />

                    {move || {
                        if !ready.get() {
                            return view! { <Skeleton rows=3 label=t!(SetupLoading) /> }.into_any();
                        }
                        match step.get() {
                        Step::Container => view! { <ContainerStep ctx=ctx /> }.into_any(),
                        Step::Source => view! { <SourceStep ctx=ctx /> }.into_any(),
                        Step::Jira => view! { <JiraStep ctx=ctx /> }.into_any(),
                        Step::GitHub => view! { <GitHubStep ctx=ctx /> }.into_any(),
                        Step::Projects => view! { <ProjectsStep ctx=ctx /> }.into_any(),
                        Step::Model => view! { <ModelStep ctx=ctx /> }.into_any(),
                        Step::Live => view! { <LiveStep ctx=ctx /> }.into_any(),
                        Step::Done => {
                            view! { <DoneStep ctx=ctx to_step=jump /> }.into_any()
                        }
                        }
                    }}

                    <HStack css="justify-content:space-between;flex-wrap:wrap">
                        <Button
                            disabled=Signal::derive(move || step.get().previous_for(from.get()).is_none())
                            on_click=back
                        >
                            {t!(SetupBack)}
                        </Button>
                        {move || {
                            step
                                .get()
                                .next_for(from.get())
                                .map(|_| {
                                    view! {
                                        <HStack spacing="12px">
                                            <Button variant=ButtonStyle::Borderless on_click=next_step>
                                                {t!(SetupSkip)}
                                            </Button>
                                            <Button variant=ButtonStyle::GlassProminent on_click=next_step>
                                                {t!(SetupNext)}
                                            </Button>
                                        </HStack>
                                    }
                                })
                        }}
                    </HStack>
                </VStack>
            </Container>
        </div>
    }
}
