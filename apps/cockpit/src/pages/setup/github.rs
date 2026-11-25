//! Step 3: connect GitHub. The default is a fine-grained personal access token; the own
//! GitHub App (OAuth) is the second way. Both end in the same list of repos and the same test.

use cockpit_core::api::{ClientInput, ConfigChange, GitHubCheck, RepoState};
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::client_id_read;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::auth_method::{AuthMethodPicker, UnusedSignInNote, current, persist_pick};
use super::fields::{
    Context, CopyField, Extern, Feedback, Intro, KeyField, Message, Section, config_save,
    error_text,
};
use super::optional::Optional;
use super::repo_picker::RepoPicker;
use super::watched_repos::WatchedRepos;
use crate::access_hint::{AccessHint, problem_of, use_github_access};
use crate::oauth_card::{OAuthCard, Service, Service as OAuthService};
use crate::server;

/// Where a fine-grained token is created.
const TOKEN_PAGE: &str = "https://github.com/settings/personal-access-tokens/new";

/// Rights of the token: (area, level) as catalog keys.
const TOKEN_RIGHTS: &[(Key, Key)] = &[
    (Key::SetupGithubAreaContents, Key::SetupGithubLevelReadWrite),
    (
        Key::SetupGithubAreaPullRequests,
        Key::SetupGithubLevelReadWrite,
    ),
    (Key::SetupGithubAreaIssues, Key::SetupGithubLevelReadWrite),
    (
        Key::SetupGithubAreaCommitStatuses,
        Key::SetupGithubLevelReadOnly,
    ),
    (
        Key::SetupGithubAreaMetadata,
        Key::SetupGithubLevelReadOnlyAlways,
    ),
];

/// Permissions of the GitHub App: (area, level) as catalog keys.
const APP_RIGHTS: &[(Key, Key)] = &[
    (
        Key::SetupGithubAreaContents,
        Key::SetupGithubLevelReadWriteApp,
    ),
    (
        Key::SetupGithubAreaPullRequests,
        Key::SetupGithubLevelReadWriteApp,
    ),
    (
        Key::SetupGithubAreaIssues,
        Key::SetupGithubLevelReadWriteApp,
    ),
    (
        Key::SetupGithubAreaChecks,
        Key::SetupGithubLevelReadWriteApp,
    ),
    (
        Key::SetupGithubAreaCommitStatuses,
        Key::SetupGithubLevelReadOnly,
    ),
    (
        Key::SetupGithubAreaMetadata,
        Key::SetupGithubLevelReadOnlyPreselected,
    ),
];

/// An entry to tick off. The tick only lives on the page, it is not saved.
#[component]
fn Checkmark(area: String, level: String) -> impl IntoView {
    let an = RwSignal::new(false);
    view! {
        <li>
            <label class="ck-check">
                <input type="checkbox" prop:checked=move || an.get() on:change=move |_| an.update(|a| *a = !*a) />
                <span style:text-decoration=move || an.get().then_some("line-through")>
                    <strong>{area}</strong>
                </span>
            </label>
            <span class="ui-checklist__permission">{level}</span>
        </li>
    }
}

#[component]
fn Rights(list: &'static [(Key, Key)]) -> impl IntoView {
    view! {
        <ul class="ui-checklist">
            {list
                .iter()
                .map(|&(b, s)| view! { <Checkmark area=tr(b, &[]) level=tr(s, &[]) /> })
                .collect_view()}
        </ul>
    }
}

/// The way with a token: instructions and a password field.
#[component]
fn TokenWay(ctx: Context) -> impl IntoView {
    view! {
        <Section title=t!(SetupGithubTokenS1)>
            <ol class="ui-instructions">
                <li>
                    {t!(SetupGithubOpen)}
                    " "
                    <Extern href=TOKEN_PAGE>"github.com/settings/personal-access-tokens/new"</Extern>
                    " "
                    {t!(SetupGithubTokenOpenRest)}
                </li>
                <li>
                    {t!(SetupGithubTokenAccess)}
                </li>
                <li>
                    {t!(SetupGithubTokenPermissions)}
                    <Rights list=TOKEN_RIGHTS />
                    <p class="t-footnote t-secondary">{t!(SetupGithubTokenNoChecks)}</p>
                    <p class="t-footnote t-secondary">{t!(SetupGithubTokenReadOnly)}</p>
                </li>
                <li>{t!(SetupGithubTokenGenerate)}</li>
            </ol>
        </Section>
        <Section title=t!(SetupGithubTokenS2)>
            <KeyField
                ctx=ctx
                name="github"
                label=t!(SetupGithubTokenLabel)
                hint=t!(SetupGithubTokenHint)
            />
        </Section>
    }
}

/// The way with an own GitHub App: create it, one group for client ID and secret, install, sign in.
#[component]
fn AppWay(ctx: Context) -> impl IntoView {
    let client_id = RwSignal::new(String::new());
    Effect::new(move |_| {
        let updated = ctx
            .setup
            .with(|e| e.github_client_id.clone().unwrap_or_default());
        if client_id.get_untracked().is_empty() && !updated.is_empty() {
            client_id.set(updated);
        }
    });
    let secret = RwSignal::new(String::new());
    let back: Feedback = RwSignal::new(None);
    let running = RwSignal::new(false);
    let callback = Signal::derive(move || ctx.setup.with(|e| e.callback_github.clone()));
    let secret_set = move || ctx.setup.with(|e| e.has_key("github-oauth"));

    // One button for both values: the ID goes to config.toml, the secret to the .env file.
    let save = move |_| {
        let id = match client_id_read(&client_id.get_untracked()) {
            Ok(id) if id.is_empty() => {
                back.set(Some((false, t!(SetupGithubClientIdMissing))));
                return;
            }
            Ok(id) => id,
            Err(m) => {
                back.set(Some((false, m.render(lang_untracked()))));
                return;
            }
        };
        let typed = secret.get_untracked();
        let typed = typed.trim().to_string();
        let to_store = if typed.is_empty() {
            if !secret_set() {
                back.set(Some((false, t!(SetupGithubSecretMissing))));
                return;
            }
            None
        } else {
            match cockpit_core::setup::key_verify(&typed) {
                Ok(v) => Some(v),
                Err(m) => {
                    back.set(Some((false, m.render(lang_untracked()))));
                    return;
                }
            }
        };
        // From here on the secret only lives in this one request.
        secret.set(String::new());
        running.set(true);
        back.set(None);
        spawn_local(async move {
            let change = ConfigChange {
                github_oauth: Some(ClientInput { client_id: id }),
                ..Default::default()
            };
            let result = async {
                let answer = server::put_config(&change).await?;
                if let Some(v) = to_store {
                    server::put_key("github-oauth", v).await?;
                }
                Ok::<_, server::CallError>(answer)
            }
            .await;
            match result {
                Ok(answer) => {
                    let mut text = t!(SetupGithubAppSaved);
                    if let Some(h) = answer.hint {
                        text.push(' ');
                        text.push_str(&h.render(lang_untracked()));
                    }
                    back.set(Some((true, text)));
                    ctx.reload();
                }
                Err(e) => back.set(Some((false, error_text(&e)))),
            }
            running.set(false);
        });
    };
    let remove = move |_| {
        client_id.set(String::new());
        config_save(
            ctx,
            ConfigChange {
                github_oauth: Some(ClientInput {
                    client_id: String::new(),
                }),
                ..Default::default()
            },
            back,
            t!(SetupGithubClientIdRemoved),
            || {},
        );
    };

    view! {
        <Section title=t!(SetupGithubAppS1)>
            <ol class="ui-instructions">
                <li>
                    {t!(SetupGithubOpen)}
                    " "
                    <Extern href="https://github.com/settings/apps/new">"github.com/settings/apps/new"</Extern>
                    " "
                    {t!(SetupGithubAppOpenRest)}
                </li>
                <li>
                    {t!(SetupGithubAppName)}
                    " "
                    <code>"https://github.com"</code>
                    " "
                    {t!(SetupGithubAppNameRest)}
                </li>
                <li>
                    {t!(SetupGithubAppCallback)}
                    <CopyField store=ctx.store label=t!(SetupGithubCallbackUrl) text=callback />
                </li>
                <li>
                    {t!(SetupGithubAppWebhook)}
                </li>
                <li>
                    {t!(SetupGithubAppPermissions)}
                    <Rights list=APP_RIGHTS />
                </li>
                <li>
                    {t!(SetupGithubAppWhere)}
                </li>
            </ol>
        </Section>
        <Section title=t!(SetupGithubAppS2)>
            <ol class="ui-instructions">
                <li>
                    {t!(SetupGithubAppStep1)}
                </li>
                <li>
                    {t!(SetupGithubAppStep2)}
                </li>
                <li>
                    {t!(SetupGithubAppStep3)}
                </li>
            </ol>
            <TextField
                label=t!(SetupGithubClientId)
                value=client_id
                hint=t!(SetupGithubClientIdHint)
                autocomplete="off"
            />
            <TextField
                label=t!(SetupGithubSecretLabel)
                value=secret
                input_type="password"
                autocomplete="off"
                hint=t!(SetupGithubSecretHint)
            />
            <HStack class="ui-gap-12">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=Signal::derive(move || running.get())
                    on_click=save
                >
                    {t!(SetupGithubSave)}
                </Button>
                <Button
                    variant=ButtonStyle::Borderless
                    disabled=Signal::derive(move || {
                        running.get() || ctx.setup.with(|e| e.github_client_id.is_none())
                    })
                    on_click=remove
                >
                    {t!(SetupGithubClientIdRemove)}
                </Button>
                <Status tone=Signal::derive(move || {
                    if secret_set() { StatusTone::Green } else { StatusTone::Gray }
                })>
                    {move || if secret_set() { t!(SetupGithubSecretStored) } else { t!(SetupGithubSecretMissingStatus) }}
                </Status>
            </HStack>
            <Message feedback=back />
        </Section>
        <Section title=t!(SetupGithubAppS3)>
            <ol class="ui-instructions">
                <li>{t!(SetupGithubInstall1)}</li>
                <li>
                    {t!(SetupGithubInstall2)}
                </li>
            </ol>
        </Section>
        <Section title=t!(SetupGithubSignInTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupGithubSignInText)}
            </p>
            <OAuthCard
                service=Service::GitHub
                on_connected=move |_| ctx.reload()
                on_disconnected=move |_| ctx.reload()
            />
        </Section>
    }
}

/// "Check connection": user and chosen repos, read-only.
#[component]
fn ConnectionCheck(ctx: Context) -> impl IntoView {
    let result: RwSignal<Option<GitHubCheck>> = RwSignal::new(None);
    let feedback: Feedback = RwSignal::new(None);
    let running = RwSignal::new(false);
    let go = move |_| {
        let repos = ctx.setup.with(|s| s.github_repos.clone());
        running.set(true);
        feedback.set(None);
        spawn_local(async move {
            match server::post_github_check(repos).await {
                Ok(a) => result.set(Some(a)),
                Err(e) => {
                    result.set(None);
                    feedback.set(Some((false, error_text(&e))));
                }
            }
            running.set(false);
        });
    };
    view! {
        <Section title=t!(SetupGithubCheckTitle)>
            <p class="t-subheadline t-secondary">
                {t!(SetupGithubCheckText)}
            </p>
            <HStack class="ui-gap-12">
                <Button on_click=go disabled=Signal::derive(move || running.get())>
                    {move || if running.get() { t!(SetupGithubChecking) } else { t!(SetupGithubCheckButton) }}
                </Button>
            </HStack>
            <Message feedback=feedback />
            {move || {
                result
                    .get()
                    .map(|a| {
                        let tone = a.ok;
                        view! {
                            <p class="ui-hint" class:ck-problem=!tone role="status">
                                {a.message.render(lang())}
                            </p>
                            <ul class="ui-checklist">
                                {a.repos
                                    .into_iter()
                                    .map(|c| {
                                        let mark = match c.state {
                                            RepoState::Ok => "✓",
                                            RepoState::ReadOnly => "!",
                                            _ => "✗",
                                        };
                                        view! { <li>{t!(SetupGithubRepoLine, mark = mark, repo = c.repo, message = c.message.render(lang()))}</li> }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                    })
            }}
        </Section>
    }
}

#[component]
pub fn GitHubStep(ctx: Context) -> impl IntoView {
    // Someone who already set up an app keeps seeing it; everyone else starts with the token.
    let chosen: RwSignal<Option<AuthKind>> = RwSignal::new(None);
    let status = Signal::derive(move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .map(|h| h.sign_in.github.clone())
                .unwrap_or_default()
        })
    });
    let default = Signal::derive(move || {
        if let Some(c) = status.with(|s| s.choice) {
            return c.into();
        }
        let app = ctx
            .setup
            .with(|s| s.github_client_id.is_some() && !s.has_key("github"));
        if app {
            AuthKind::OAuth
        } else {
            AuthKind::Token
        }
    });
    let method = current(chosen, default);
    let (access, recheck) = use_github_access();
    let watch_opened = RwSignal::new(false);
    let settings_opened = RwSignal::new(false);

    view! {
        <VStack class="ui-gap-12">
            <Intro title=t!(SetupGithubTitle)>
                <p>{t!(SetupGithubIntro)}</p>
            </Intro>
            <AuthMethodPicker
                chosen=chosen
                default=default
                aria_label=t!(SetupGithubPickerAria)
                on_pick=Callback::new(move |kind: AuthKind| {
                    let ready = if kind == AuthKind::OAuth {
                        status.with(|s| s.oauth_stored)
                    } else {
                        ctx.setup.with(|s| s.has_key("github"))
                    };
                    persist_pick(ctx, OAuthService::GitHub, kind, ready);
                })
            />
            <UnusedSignInNote status=status />
            <p class="t-footnote t-secondary">{t!(SetupGithubTokenIfNoApp)}</p>
            <div hidden=move || method.get() != AuthKind::Token>
                <VStack class="ui-gap-12">
                    <TokenWay ctx=ctx />
                </VStack>
            </div>
            <div hidden=move || method.get() != AuthKind::OAuth>
                <VStack class="ui-gap-12">
                    <AppWay ctx=ctx />
                </VStack>
            </div>
            <Optional title=t!(SetupReposWatchTitle) opened=watch_opened>
                <WatchedRepos ctx=ctx opened=watch_opened />
            </Optional>
            <Optional title=t!(SetupReposTitle) opened=settings_opened>
                <RepoPicker ctx=ctx opened=settings_opened />
            </Optional>
            {move || problem_of(&access).map(|a| view! { <AccessHint access=a recheck=recheck /> })}
            <ConnectionCheck ctx=ctx />
        </VStack>
    }
}
