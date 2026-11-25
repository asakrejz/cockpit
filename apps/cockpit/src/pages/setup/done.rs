//! Step 5: summary with ticks and plain text.

use cockpit_core::AuthKind;
use cockpit_core::setup::{Snapshot, Step, Tone, summary};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Intro};
use crate::access_hint::{AccessHint, problem_of, use_github_access};
use crate::server;
use crate::sync_status::SyncStatus;
use crate::view::setup_tone;

/// {t!(SetupDoneRenewToken)} and {t!(SetupDoneDisconnect)} for Jira. Disconnecting deletes the token in the .env file (with
/// OAuth: revokes the sign-in); the settings stay, the way back is the step "Jira".
#[component]
fn JiraActions(ctx: Context, to_step: Callback<Step>) -> impl IntoView {
    let kind = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().map(|h| h.sign_in.jira.kind))
    };
    let disconnect = move |_| {
        let oauth = kind() == Some(AuthKind::OAuth);
        spawn_local(async move {
            let result = if oauth {
                server::post_revoke("atlassian").await.map(|_| ())
            } else {
                server::delete_key("jira").await.map(|_| ())
            };
            match result {
                Ok(()) => ctx.store.notify(t!(SetupDoneJiraDisconnected)),
                Err(e) => ctx.store.notify(e.message()),
            }
            ctx.reload();
        });
    };
    view! {
        {move || {
            (kind() == Some(AuthKind::Token))
                .then(|| {
                    view! {
                        <Button
                            size=ControlSize::Small
                            on_click=move |_| to_step.run(Step::Jira)
                        >
                            {t!(SetupDoneRenewToken)}
                        </Button>
                    }
                })
        }}
        <Button size=ControlSize::Small destructive=true on_click=disconnect>
            {t!(SetupDoneDisconnect)}
        </Button>
    }
}

#[component]
pub fn DoneStep(ctx: Context, to_step: Callback<Step>) -> impl IntoView {
    let (access, recheck) = use_github_access();
    let lines = move || {
        let health = ctx.store.health.get().unwrap_or_default();
        summary(&Snapshot {
            setup: ctx.setup.get(),
            jira: health.sign_in.jira.clone(),
            github: health.sign_in.github.clone(),
            endpoint: health.endpoint.clone(),
            container: health.container.clone(),
            agent: health.agent.clone(),
            check: ctx.check.get(),
            github_access: access.get(),
        })
    };
    view! {
        <VStack class="ui-gap-12">
            <Intro title=t!(SetupDoneTitle)>
                <p>
                    {t!(SetupDoneIntro)}
                </p>
            </Intro>
            <List>
                <ListSection header=t!(SetupDoneHeader)>
                    {move || {
                        lines()
                            .into_iter()
                            .map(|z| {
                                let (label, symbol) = match z.tone {
                                    Tone::Good => (t!(SetupDoneToneGood), "✓ "),
                                    Tone::Open => (t!(SetupDoneToneOpen), ""),
                                    Tone::Problem => (t!(SetupDoneToneProblem), "! "),
                                };
                                let tone = setup_tone(z.tone);
                                let step = z.step;
                                let jira_connected = z.step == Step::Jira && z.tone == Tone::Good;
                                view! {
                                    <ListRow title=format!("{symbol}{}", z.name.render(lang())) subtitle=z.text.render(lang()) wrap=true>
                                        <Status tone=tone>{label}</Status>
                                        {(z.tone != Tone::Good || z.step == Step::Source)
                                            .then(|| {
                                                view! {
                                                    <Button
                                                        size=ControlSize::Small
                                                        on_click=move |_| to_step.run(step)
                                                    >
                                                        {t!(SetupDoneView)}
                                                    </Button>
                                                }
                                            })}
                                        {jira_connected
                                            .then(|| view! { <JiraActions ctx=ctx to_step=to_step /> })}
                                    </ListRow>
                                }
                            })
                            .collect_view()
                    }}
                </ListSection>
            </List>
            {move || problem_of(&access).map(|a| view! { <AccessHint access=a recheck=recheck /> })}
            <SyncStatus />
            <HStack class="ui-justify-end">
                <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href=crate::route::Route::TASKS_HASH>
                    {t!(SetupDoneToDashboard)}
                </Button>
            </HStack>
            <p class="t-footnote t-secondary ui-px-4">
                {t!(SetupDoneKeysNote)}
            </p>
            {move || {
                ctx.store
                    .health
                    .get()
                    .and_then(|h| h.unreadable_state)
                    .map(|kept| {
                        view! {
                            <p class="t-footnote ck-problem ui-px-4" role="alert">
                                {kept.notice.render(lang())}
                            </p>
                        }
                    })
            }}
            {move || {
                ctx.store
                    .health
                    .get()
                    .and_then(|h| h.env_file)
                    .map(|file| {
                        view! {
                            <p class="t-footnote t-secondary ui-px-4">
                                {t!(SetupDoneEnvFile)} " "
                                <code>{file.path}</code>
                            </p>
                            {file.warnings
                                .iter()
                                .map(|w| {
                                    view! {
                                        <p class="t-footnote ui-px-4 ck-problem" role="alert">
                                            {w.render(lang())}
                                        </p>
                                    }
                                })
                                .collect_view()}
                        }
                    })
            }}
            {move || {
                ctx.store
                    .health
                    .get()
                    .and_then(|h| h.config_files)
                    .map(|folder| {
                        view! {
                            <p class="t-footnote t-secondary ui-px-4">
                                {t!(SetupDoneConfigFiles)} " "
                                <code>{folder.path}</code>
                            </p>
                            {folder.warnings
                                .iter()
                                .map(|w| {
                                    view! {
                                        <p class="t-footnote ui-px-4 ck-problem" role="alert">
                                            {w.render(lang())}
                                        </p>
                                    }
                                })
                                .collect_view()}
                        }
                    })
            }}
        </VStack>
    }
}
