//! Step 2: connect Jira – a picker chooses the way, only that way is shown: the API token (three
//! simple steps, then a connection test and the choice of the project) or the Atlassian app
//! (OAuth). The rules (read address, texts) are in `cockpit_core::setup`.

use cockpit_core::api::JiraCheck;
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::jira_default_method;
use feature_ui::*;
use leptos::prelude::*;

use super::auth_method::{AuthMethodPicker, UnusedSignInNote, current, persist_pick};
use super::fields::{Context, Intro};

mod oauth;
mod project_choice;
mod token;

use oauth::JiraOauth;
use project_choice::JiraProjectChoice;
use token::JiraToken;

/// Fills a field from the server as long as it is still empty (the user never types against it).
fn prefill(field: RwSignal<String>, source: impl Fn() -> String + 'static) {
    Effect::new(move |_| {
        let updated = source();
        if field.get_untracked().is_empty() && !updated.is_empty() {
            field.set(updated);
        }
    });
}

#[component]
pub fn JiraStep(ctx: Context) -> impl IntoView {
    // The result of the connection test; only after it does the choice of the project appear.
    let result: RwSignal<Option<JiraCheck>> = RwSignal::new(None);
    let per_token = move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.sign_in.jira.connected && h.sign_in.jira.kind == AuthKind::Token)
        })
    };
    // Which way is shown: the one the user picked, until then the connected one (else the token).
    let chosen = RwSignal::new(None::<AuthKind>);
    let default = Signal::derive(move || {
        let status = ctx.store.health.with(|h| {
            h.as_ref()
                .map(|h| h.sign_in.jira.clone())
                .unwrap_or_default()
        });
        match status.choice {
            Some(c) => c.into(),
            None => {
                jira_default_method(&status, ctx.setup.with(|e| e.atlassian_client_id.is_some()))
            }
        }
    });
    let status = Signal::derive(move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .map(|h| h.sign_in.jira.clone())
                .unwrap_or_default()
        })
    });
    let method = current(chosen, default);
    // Both ways stay in the page (only hidden), so what was typed in one is still there after
    // switching back and forth.
    view! {
        <VStack class="ui-gap-12">
            <Intro title=t!(SetupJiraTitle)>
                <p>{t!(SetupJiraIntro)}</p>
                <p>
                    <strong>{t!(SetupJiraGoodToKnow)}</strong>
                    " "
                    {t!(SetupJiraGoodToKnowText)}
                </p>
            </Intro>
            <AuthMethodPicker
                chosen=chosen
                default=default
                aria_label=t!(SetupJiraPickerAria)
                on_pick=Callback::new(move |kind: AuthKind| {
                    let ready = if kind == AuthKind::OAuth {
                        status.with(|s| s.oauth_stored)
                    } else {
                        ctx.setup.with(|s| s.has_key("jira"))
                    };
                    persist_pick(ctx, crate::oauth_card::Service::Atlassian, kind, ready);
                })
            />
            <UnusedSignInNote status=status />
            <div hidden=move || method.get() != AuthKind::Token>
                <VStack class="ui-gap-12">
                    <JiraToken ctx=ctx result=result />
                    {move || {
                        (result.with(|e| e.as_ref().is_some_and(|e| e.ok)) || per_token())
                            .then(|| view! { <JiraProjectChoice ctx=ctx /> })
                    }}
                </VStack>
            </div>
            <div hidden=move || method.get() != AuthKind::OAuth>
                <VStack class="ui-gap-12">
                    <Intro title=t!(SetupJiraOauthTitle)>
                        <p>{t!(SetupJiraOauthIntro)}</p>
                    </Intro>
                    <JiraOauth ctx=ctx />
                </VStack>
            </div>
        </VStack>
    }
}
