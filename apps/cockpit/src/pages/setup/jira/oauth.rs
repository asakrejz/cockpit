//! Jira via the Atlassian app (OAuth): client ID and secret, site, projects and the sign-in card.

use cockpit_core::api::{ClientInput, ConfigChange, JiraInput};
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::{client_id_read, projects_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::prefill;
use crate::oauth_card::{OAuthCard, Service, forget_client};
use crate::pages::setup::fields::{
    Context, CopyField, Extern, Feedback, KeyField, Message, Section, config_save, error_text,
};

#[component]
pub(super) fn JiraOauth(ctx: Context) -> impl IntoView {
    let client_id = RwSignal::new(String::new());
    prefill(client_id, move || {
        ctx.setup
            .with(|e| e.atlassian_client_id.clone().unwrap_or_default())
    });
    let projects = RwSignal::new(String::new());
    prefill(projects, move || {
        ctx.setup.with(|e| e.jira_projects.join(", "))
    });
    let site = RwSignal::new(String::new());
    prefill(site, move || {
        ctx.setup
            .with(|e| e.atlassian_site.clone().unwrap_or_default())
    });
    let back_id: Feedback = RwSignal::new(None);
    let back_site: Feedback = RwSignal::new(None);
    let back_projects: Feedback = RwSignal::new(None);
    // Asking before the client ID and secret are removed.
    let confirm_remove = RwSignal::new(false);
    let removing = RwSignal::new(false);
    // Bumped after removing, so that the sign-in card below asks the server anew.
    let card_generation = RwSignal::new(0u32);
    let id_stored = Memo::new(move |_| ctx.setup.with(|e| e.atlassian_client_id.is_some()));
    let oauth_connected = Memo::new(move |_| {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.sign_in.jira.connected && h.sign_in.jira.kind == AuthKind::OAuth)
        })
    });
    let callback = Signal::derive(move || ctx.setup.with(|e| e.callback_atlassian.clone()));

    let id_save = move |_| match client_id_read(&client_id.get_untracked()) {
        Ok(id) if id.is_empty() => back_id.set(Some((false, t!(SetupJiraClientIdMissing)))),
        Ok(id) => config_save(
            ctx,
            ConfigChange {
                atlassian: Some(ClientInput { client_id: id }),
                ..Default::default()
            },
            back_id,
            t!(SetupJiraClientIdSaved),
            || {},
        ),
        Err(m) => back_id.set(Some((false, m.render(lang_untracked())))),
    };
    let site_save = move |_| {
        config_save(
            ctx,
            ConfigChange {
                atlassian_site: Some(site.get_untracked()),
                ..Default::default()
            },
            back_site,
            t!(SetupJiraSiteSaved),
            || {},
        )
    };
    let remove = move |_| {
        removing.set(true);
        back_id.set(None);
        spawn_local(async move {
            match forget_client(Service::Atlassian).await {
                Ok(()) => {
                    client_id.set(String::new());
                    site.set(String::new());
                    back_id.set(Some((true, t!(SetupJiraClientRemoved))));
                    card_generation.update(|n| *n += 1);
                    ctx.reload();
                }
                Err(e) => back_id.set(Some((false, error_text(&e)))),
            }
            confirm_remove.set(false);
            removing.set(false);
        });
    };
    let projects_save = move |_| {
        config_save(
            ctx,
            ConfigChange {
                jira: Some(JiraInput {
                    projects: Some(projects_read(&projects.get_untracked())),
                    ..Default::default()
                }),
                ..Default::default()
            },
            back_projects,
            t!(SetupJiraProjectsSaved),
            || {},
        )
    };

    view! {
        <Section title=t!(SetupJiraOauthS1)>
            <ol class="ui-instructions">
                <li>
                    {t!(SetupJiraOauthOpen)}
                    " "
                    <Extern href="https://developer.atlassian.com/console/myapps/">
                        "developer.atlassian.com/console/myapps"
                    </Extern>
                    " "
                    {t!(SetupJiraOauthOpenRest)}
                </li>
                <li>
                    {t!(SetupJiraOauthCreate)}
                </li>
                <li>
                    {t!(SetupJiraOauthPermissions)}
                </li>
                <li>
                    {t!(SetupJiraOauthScopes)}
                    <ul class="ui-checklist">
                        <li><code>"read:jira-work"</code><span class="ui-checklist__permission">{t!(SetupJiraScopeRead)}</span></li>
                        <li><code>"write:jira-work"</code><span class="ui-checklist__permission">{t!(SetupJiraScopeWrite)}</span></li>
                        <li><code>"read:jira-user"</code><span class="ui-checklist__permission">{t!(SetupJiraScopeUser)}</span></li>
                    </ul>
                </li>
                <li>
                    {t!(SetupJiraOauthAuthorization)}
                    <CopyField store=ctx.store label=t!(SetupJiraCallbackUrl) text=callback />
                </li>
                <li>
                    {t!(SetupJiraOauthSettings)}
                </li>
            </ol>
        </Section>
        <Section title=t!(SetupJiraOauthS2)>
            <TextField
                label=t!(SetupJiraClientId)
                value=client_id
                hint=t!(SetupJiraClientIdHint)
                autocomplete="off"
            />
            <HStack class="ui-gap-12">
                <Button variant=ButtonStyle::GlassProminent on_click=id_save>
                    {t!(SetupJiraClientIdSave)}
                </Button>
                {move || {
                    id_stored
                        .get()
                        .then(|| {
                            view! {
                                <Button
                                    destructive=true
                                    disabled=Signal::derive(move || oauth_connected.get())
                                    on_click=move |_| confirm_remove.set(true)
                                >
                                    {t!(SetupJiraClientIdRemove)}
                                </Button>
                            }
                        })
                }}
            </HStack>
            {move || {
                (id_stored.get() && oauth_connected.get())
                    .then(|| {
                        view! {
                            <p class="ui-hint">
                                {t!(SetupJiraRemoveFirst)}
                            </p>
                        }
                    })
            }}
            {move || {
                confirm_remove
                    .get()
                    .then(|| {
                        view! {
                            <div class="ui-hint" role="alertdialog" aria-label=t!(SetupJiraClientIdRemove)>
                                <p>{t!(SetupJiraRemoveConfirm)}</p>
                                <HStack class="ui-gap-12 ui-mt-8">
                                    <Button destructive=true disabled=removing on_click=remove>
                                        {t!(SetupJiraRemoveYes)}
                                    </Button>
                                    <Button on_click=move |_| confirm_remove.set(false)>
                                        {t!(SetupJiraCancel)}
                                    </Button>
                                </HStack>
                            </div>
                        }
                    })
            }}
            <Message feedback=back_id />
            <KeyField
                ctx=ctx
                name="atlassian-oauth"
                label=t!(SetupJiraSecretLabel)
                hint=t!(SetupJiraSecretHint)
            />
        </Section>
        <Section title=t!(SetupJiraSiteTitle)>
            <TextField
                label=t!(SetupJiraSiteLabel)
                value=site
                hint=t!(SetupJiraSiteHint)
                autocomplete="off"
            />
            <HStack class="ui-gap-12">
                <Button
                    on_click=site_save
                    disabled=Signal::derive(move || !id_stored.get())
                >
                    {t!(SetupJiraSiteSave)}
                </Button>
            </HStack>
            <Message feedback=back_site />
        </Section>
        <Section title=t!(SetupJiraProjectsTitle)>
            <TextField
                label=t!(SetupJiraProjectsLabel)
                value=projects
                hint=t!(SetupJiraProjectsHint)
                autocomplete="off"
            />
            <HStack class="ui-gap-12">
                <Button on_click=projects_save>{t!(SetupJiraProjectsSave)}</Button>
            </HStack>
            <Message feedback=back_projects />
        </Section>
        <Section title=t!(SetupJiraSignInTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupJiraSignInText)}
            </p>
            {move || {
                card_generation.track();
                view! {
                    <OAuthCard
                        service=Service::Atlassian
                        on_connected=move |_| ctx.reload()
                        on_disconnected=move |_| ctx.reload()
                    />
                }
            }}
        </Section>
    }
}
