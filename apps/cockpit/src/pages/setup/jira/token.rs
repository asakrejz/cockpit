//! Jira via API token: address, email, token and the connection test.

use cockpit_core::api::{ConfigChange, JiraCheck, JiraInput};
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::{JIRA_TOKEN_PAGE, jira_site_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::prefill;
use crate::pages::setup::fields::{Context, Feedback, KeyField, Message, Section, config_save};
use crate::server;

/// A link in the look of a button that opens in a new tab.
#[component]
fn ExternButton(#[prop(into)] href: String, children: Children) -> impl IntoView {
    view! {
        <a
            class="ui-button"
            data-style="glass-prominent"
            href=href
            target="_blank"
            rel="noopener noreferrer"
        >
            {children()}
        </a>
    }
}

#[component]
pub(super) fn JiraToken(ctx: Context, result: RwSignal<Option<JiraCheck>>) -> impl IntoView {
    let site = RwSignal::new(String::new());
    prefill(site, move || ctx.setup.with(|e| e.jira_site.clone()));
    let email = RwSignal::new(String::new());
    prefill(email, move || ctx.setup.with(|e| e.jira_email.clone()));
    let back: Feedback = RwSignal::new(None);
    let running = RwSignal::new(false);

    // If saving fails, the test does not run: release the button again.
    Effect::new(move |_| {
        if back.with(|r| r.as_ref().is_some_and(|(ok, _)| !ok)) {
            running.set(false);
        }
    });

    // This is how Cockpit understands the input: visible right below the field.
    let site_read = Signal::derive(move || jira_site_read(&site.get()));
    let connected = move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.sign_in.jira.connected && h.sign_in.jira.kind == AuthKind::Token)
        })
    };

    let check = move |_| {
        result.set(None);
        back.set(None);
        let site_new = match site_read.get_untracked() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                back.set(Some((false, t!(SetupJiraSiteMissing))));
                return;
            }
            Err(m) => {
                back.set(Some((false, m.render(lang_untracked()))));
                return;
            }
        };
        let email_new = email.get_untracked().trim().to_string();
        if !email_new.contains('@') || email_new.contains(char::is_whitespace) {
            back.set(Some((false, t!(SetupJiraEmailMissing))));
            return;
        }
        running.set(true);
        // First secure address and email, then the test (read only).
        config_save(
            ctx,
            ConfigChange {
                jira: Some(JiraInput {
                    site: Some(site_new),
                    email: Some(email_new),
                    projects: None,
                }),
                ..Default::default()
            },
            back,
            t!(SetupJiraAddressSaved),
            move || {
                spawn_local(async move {
                    match server::post_jira_check().await {
                        Ok(p) => result.set(Some(p)),
                        Err(e) => result.set(Some(JiraCheck {
                            message: cockpit_i18n::Msg::raw(e.message()),
                            ..Default::default()
                        })),
                    }
                    ctx.reload();
                    running.set(false);
                });
            },
        );
    };

    view! {
        <Section title=t!(SetupJiraTokenS1)>
            <TextField
                label=t!(SetupJiraAddressLabel)
                value=site
                placeholder=t!(SetupJiraAddressPlaceholder)
                hint=t!(SetupJiraAddressHint)
                autocomplete="off"
            />
            {move || match site_read.get() {
                Ok(s) if !s.is_empty() => {
                    view! { <p class="ui-hint">{t!(SetupJiraUses)} " " <code>{s}</code></p> }.into_any()
                }
                Ok(_) => ().into_any(),
                Err(m) => {
                    view! { <p class="ui-hint ck-problem" role="alert">{m.render(lang())}</p> }
                        .into_any()
                }
            }}
        </Section>
        <Section title=t!(SetupJiraTokenS2)>
            <TextField
                label=t!(SetupJiraEmailLabel)
                value=email
                input_type="email"
                hint=t!(SetupJiraEmailHint)
                autocomplete="off"
            />
        </Section>
        <Section title=t!(SetupJiraTokenS3)>
            <ol class="ui-instructions">
                <li>
                    {t!(SetupJiraTokenStep1)}
                    <HStack class="ui-gap-12 ui-mt-8">
                        <ExternButton href=JIRA_TOKEN_PAGE>{t!(SetupJiraTokenPage)}</ExternButton>
                    </HStack>
                </li>
                <li>{t!(SetupJiraTokenStep2)}</li>
                <li>
                    {t!(SetupJiraTokenStep3)}
                </li>
                <li>
                    {t!(SetupJiraTokenStep4)}
                </li>
            </ol>
            <KeyField
                ctx=ctx
                name="jira"
                label=t!(SetupJiraTokenLabel)
                hint=t!(SetupJiraTokenHint)
            />
        </Section>
        <Section title=t!(SetupJiraCheckTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupJiraCheckText)}
            </p>
            <HStack class="ui-gap-12">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=running
                    on_click=check
                >
                    {t!(SetupJiraCheckButton)}
                </Button>
                <Status tone=Signal::derive(move || {
                    if connected() { StatusTone::Green } else { StatusTone::Gray }
                })>{move || if connected() { t!(SetupJiraConnected) } else { t!(SetupJiraNotChecked) }}</Status>
            </HStack>
            <Message feedback=back />
            {move || {
                result
                    .get()
                    .map(|p| {
                        view! {
                            <p
                                class="ui-hint"
                                class:ck-problem=!p.ok
                                role=if p.ok { "status" } else { "alert" }
                            >
                                {if p.ok { "✓ " } else { "" }}
                                {p.message.render(lang())}
                            </p>
                        }
                    })
            }}
        </Section>
    }
}
