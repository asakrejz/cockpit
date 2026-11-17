//! Step 5: summary with ticks and plain text.

use cockpit_core::AuthKind;
use cockpit_core::setup::{Snapshot, Step, Tone, summary};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Intro};
use crate::server;
use crate::sync_status::SyncStatus;

/// "Token erneuern" and "Trennen" for Jira. Disconnecting deletes the token in the .env file (with
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
                Ok(()) => ctx
                    .store
                    .notify("Jira getrennt – der Zugang wurde gelöscht."),
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
                            "Token erneuern"
                        </Button>
                    }
                })
        }}
        <Button size=ControlSize::Small destructive=true on_click=disconnect>
            "Trennen"
        </Button>
    }
}

#[component]
pub fn DoneStep(ctx: Context, to_step: Callback<Step>) -> impl IntoView {
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
        })
    };
    view! {
        <VStack spacing="12px">
            <Intro title="Zusammenfassung">
                <p>
                    "Hier siehst du, was läuft und was noch fehlt. Offene Punkte sind kein Fehler – du kannst sie jederzeit über „Einrichten“ nachholen."
                </p>
            </Intro>
            <List>
                <ListSection header="Stand">
                    {move || {
                        lines()
                            .into_iter()
                            .map(|z| {
                                let (tone, label, symbol) = match z.tone {
                                    Tone::Good => (StatusTone::Green, "In Ordnung", "✓ "),
                                    Tone::Open => (StatusTone::Gray, "Offen", ""),
                                    Tone::Problem => (StatusTone::Red, "Prüfen", "! "),
                                };
                                let step = z.step;
                                let jira_connected = z.name == "Jira" && z.tone == Tone::Good;
                                view! {
                                    <ListRow title=format!("{symbol}{}", z.name) subtitle=z.text wrap=true>
                                        <Status tone=tone>{label}</Status>
                                        {(z.tone != Tone::Good)
                                            .then(|| {
                                                view! {
                                                    <Button
                                                        size=ControlSize::Small
                                                        on_click=move |_| to_step.run(step)
                                                    >
                                                        "Ansehen"
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
            <SyncStatus />
            <HStack css="justify-content: flex-end">
                <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href="#/">
                    "Zum Dashboard"
                </Button>
            </HStack>
            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                "Wo Schlüssel liegen: nur in der .env-Datei im Projektordner dieses Rechners. Sie gehen über die Eingabefelder hier hinein und kommen nie in den Browser-Speicher, in die Datenbank oder ins Log. Die Datei darf nie committet oder weitergegeben werden. Der echte Agent läuft in Docker oder Podman und spricht mit dem Modell-Endpunkt, den du im Schritt „Modell“ gewählt hast."
            </p>
            {move || {
                ctx.store
                    .health
                    .get()
                    .and_then(|h| h.env_file)
                    .map(|datei| {
                        view! {
                            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                                "Die Datei liegt hier: "
                                <code>{datei.path}</code>
                            </p>
                            {datei
                                .warnings
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
                                "Konfiguration und Datenbank liegen hier: "
                                <code>{ordner.path}</code>
                            </p>
                            {ordner
                                .warnings
                                .into_iter()
                                .map(|w| {
                                    view! {
                                        <p class="t-footnote" role="alert" style="padding-inline: 4px; color: var(--red, #c00)">
                                            {w}
                                        </p>
                                    }
                                })
                                .collect_view()}
                            {ordner
                                .notices
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
