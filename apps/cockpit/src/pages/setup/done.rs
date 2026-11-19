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

/// {t!(SetupDoneRenewToken)} and {t!(SetupDoneDisconnect)} for Jira. Disconnecting deletes the token in the .env file (with
/// OAuth: revokes the sign-in); the settings stay, the way back is the step "Jira".
#[component]
fn JiraActions(ctx: Context, to_step: Callback<Step>) -> impl IntoView {
    let art = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().map(|h| h.sign_in.jira.kind))
    };
    let disconnect = move |_| {
        let oauth = art() == Some(AuthKind::OAuth);
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
            (art() == Some(AuthKind::Token))
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
        <VStack spacing="12px">
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
                                let (tone, label, symbol) = match z.tone {
                                    Tone::Good => (StatusTone::Green, t!(SetupDoneToneGood), "✓ "),
                                    Tone::Open => (StatusTone::Gray, t!(SetupDoneToneOpen), ""),
                                    Tone::Problem => (StatusTone::Red, t!(SetupDoneToneProblem), "! "),
                                };
                                let step = z.step;
                                let jira_connected = z.step == Step::Jira && z.tone == Tone::Good;
                                view! {
                                    <ListRow title=format!("{symbol}{}", z.name.render(lang())) subtitle=z.text.render(lang()) wrap=true>
                                        <Status tone=tone>{label}</Status>
                                        {(z.tone != Tone::Good)
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
            <HStack css="justify-content: flex-end">
                <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href=crate::route::Route::TASKS_HASH>
                    {t!(SetupDoneToDashboard)}
                </Button>
            </HStack>
            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                {t!(SetupDoneKeysNote)}
            </p>
            {move || {
                ctx.store
                    .health
                    .get()
                    .and_then(|h| h.env_file)
                    .map(|datei| {
                        view! {
                            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                                {t!(SetupDoneEnvFile)} " "
                                <code>{datei.path}</code>
                            </p>
                            {server::shown_list(&datei.warnings, &datei.warning_msgs)
                                .into_iter()
                                .map(|w| {
                                    view! {
                                        <p class="t-footnote" role="alert" style="padding-inline: 4px; color: var(--red, #c00)">
                                            {w}
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
                    .map(|ordner| {
                        view! {
                            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                                {t!(SetupDoneConfigFiles)} " "
                                <code>{ordner.path}</code>
                            </p>
                            {server::shown_list(&ordner.warnings, &ordner.warning_msgs)
                                .into_iter()
                                .map(|w| {
                                    view! {
                                        <p class="t-footnote" role="alert" style="padding-inline: 4px; color: var(--red, #c00)">
                                            {w}
                                        </p>
                                    }
                                })
                                .collect_view()}
                            {server::shown_list(&ordner.notices, &ordner.notice_msgs)
                                .into_iter()
                                .map(|n| {
                                    view! {
                                        <p class="t-footnote t-secondary" style="padding-inline: 4px">
                                            {n}
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
