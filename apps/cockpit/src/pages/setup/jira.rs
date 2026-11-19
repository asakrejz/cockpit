//! Step 2: connect Jira – a picker chooses the way, only that way is shown: the API token (three
//! simple steps, then a connection test and the choice of the project) or the Atlassian app
//! (OAuth). The rules (read address, texts) are in `cockpit_core::setup`.

use cockpit_core::Action;
use cockpit_core::api::{ClientInput, ConfigChange, JiraCheck, JiraInput, JiraProject};
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::{
    JIRA_TOKEN_PAGE, client_id_read, jira_default_method, jira_site_read, projects_read,
};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::auth_method::{AuthMethodPicker, UnusedSignInNote, current, persist_pick};
use super::fields::{
    Context, CopyField, Extern, Feedback, Intro, KeyField, Message, Section, config_save,
    error_text,
};
use crate::oauth_card::{OAuthCard, Service, forget_client};
use crate::server::{self, CallError};

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
        <VStack spacing="12px">
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
                <VStack spacing="12px">
                    <JiraToken ctx=ctx result=result />
                    {move || {
                        (result.with(|e| e.as_ref().is_some_and(|e| e.ok)) || per_token())
                            .then(|| view! { <JiraProjectChoice ctx=ctx /> })
                    }}
                </VStack>
            </div>
            <div hidden=move || method.get() != AuthKind::OAuth>
                <VStack spacing="12px">
                    <Intro title=t!(SetupJiraOauthTitle)>
                        <p>{t!(SetupJiraOauthIntro)}</p>
                    </Intro>
                    <JiraOauth ctx=ctx />
                </VStack>
            </div>
        </VStack>
    }
}

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
fn JiraOauth(ctx: Context) -> impl IntoView {
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
            <HStack spacing="12px" css="flex-wrap:wrap">
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
                                <HStack spacing="12px" css="margin-top: 8px">
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
            <HStack spacing="12px">
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
            <HStack spacing="12px">
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

#[component]
fn JiraToken(ctx: Context, result: RwSignal<Option<JiraCheck>>) -> impl IntoView {
    let site = RwSignal::new(String::new());
    prefill(site, move || ctx.setup.with(|e| e.jira_site.clone()));
    let email = RwSignal::new(String::new());
    prefill(email, move || ctx.setup.with(|e| e.jira_email.clone()));
    let back: Feedback = RwSignal::new(None);
    let running = RwSignal::new(false);

    // If saving fails, the test does not run: release the button again.
    Effect::new(move |_| {
        if back.with(|r| r.as_ref().is_some_and(|(gut, _)| !gut)) {
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
                            message: e.message(),
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
                placeholder="firma.atlassian.net"
                hint=t!(SetupJiraAddressHint)
                autocomplete="off"
            />
            {move || match site_read.get() {
                Ok(s) if !s.is_empty() => {
                    view! { <p class="ui-hint">{t!(SetupJiraUses)} " " <code>{s}</code></p> }.into_any()
                }
                Ok(_) => ().into_any(),
                Err(m) => {
                    view! { <p class="ui-hint" role="alert" style="color: var(--sys-color-red)">{m.render(lang())}</p> }
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
                    <HStack spacing="12px" css="margin-top: 8px">
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
            <HStack spacing="12px">
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
                                role=if p.ok { "status" } else { "alert" }
                                style=(!p.ok).then_some("color: var(--sys-color-red)")
                            >
                                {if p.ok { "✓ " } else { "" }}
                                {server::shown(&p.message, p.msg.as_ref())}
                            </p>
                        }
                    })
            }}
        </Section>
    }
}

/// After the test: the Jira projects to choose from ("Alle Projekte" is possible). Chosen as in
/// the inbox with `Action::SelectJiraProject`; the list is only read.
#[component]
fn JiraProjectChoice(ctx: Context) -> impl IntoView {
    let projects = RwSignal::new(Vec::<JiraProject>::new());
    let back: Feedback = RwSignal::new(None);
    let loaded = RwSignal::new(false);
    spawn_local(async move {
        match server::fetch_jira_projects().await {
            Ok(r) => projects.set(r.projects),
            Err(CallError::Unreachable) => {
                back.set(Some((false, CallError::Unreachable.message())))
            }
            Err(e) => back.set(Some((
                false,
                t!(SetupJiraProjectsFailed, error = e.message()),
            ))),
        }
        loaded.set(true);
    });
    let options = Signal::derive(move || {
        let mut list: Vec<(String, String)> = projects.with(|p| {
            p.iter()
                .map(|p| {
                    let label = if p.name.is_empty() {
                        p.key.clone()
                    } else {
                        format!("{} · {}", p.key, p.name)
                    };
                    (p.key.clone(), label)
                })
                .collect()
        });
        list.sort_by_key(|a| a.1.to_lowercase());
        std::iter::once((String::new(), t!(SetupJiraAllProjects)))
            .chain(list)
            .collect()
    });
    let chosen = Signal::derive(move || {
        ctx.store
            .ws
            .with(|w| w.jira_project.clone().unwrap_or_default())
    });
    let switch = move |key: String| {
        let project = (!key.is_empty()).then_some(key);
        if let Err(e) = ctx.store.dispatch(Action::SelectJiraProject { project }) {
            back.set(Some((false, e.msg().render(feature_ui::lang_untracked()))));
        } else {
            back.set(Some((true, t!(SetupJiraChoiceSaved))));
        }
    };
    view! {
        <Section title=t!(SetupJiraChoiceTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupJiraChoiceText)}
            </p>
            {move || {
                (!loaded.get())
                    .then(|| view! { <p class="t-footnote t-secondary">{t!(SetupJiraLoading)}</p> })
            }}
            {move || {
                (loaded.get() && projects.with(|p| p.is_empty()) && back.with(|r| r.is_none()))
                    .then(|| {
                        view! {
                            <p class="ui-hint">
                                {t!(SetupJiraNoProjects)}
                            </p>
                        }
                    })
            }}
            <Select
                options=options
                value=chosen
                on_change=switch
                aria_label=t!(SetupJiraProjectAria)
            />
            <Message feedback=back />
        </Section>
    }
}
