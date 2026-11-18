//! Step 5: live update – optional webhooks of GitHub and Jira that make the sync faster.
//! Without them Cockpit asks every two minutes; that stays the default and works without any
//! of this. A webhook is only a signal "something changed": Cockpit then syncs right away and
//! takes no data from the request. The rules are in the server (`webhooks`) and in
//! `cockpit_core::setup`; here is only wiring.

use cockpit_core::api::{ConfigChange, WebhookSourceInfo, WebhooksInput};
use cockpit_core::setup::{
    GITHUB_WEBHOOK_EVENTS, JIRA_WEBHOOK_EVENTS, public_url_read, webhook_received_text,
    webhook_target,
};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{
    Context, CopyField, Extern, Feedback, Intro, Message, Section, command, config_save, error_text,
};
use crate::server;
use crate::store::now_ms;

/// The port of this page, for the tunnel commands.
fn port_text() -> String {
    window()
        .location()
        .port()
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "8787".into())
}

/// Secret, status and local test of one source. `id` is `github` or `jira`.
#[component]
fn SourceSection(
    ctx: Context,
    id: &'static str,
    #[prop(into)] title: String,
    #[prop(into)] info: Signal<WebhookSourceInfo>,
    children: Children,
) -> impl IntoView {
    // The secret just generated: lives only in this signal, never stored, gone when the page is left.
    let fresh = RwSignal::new(None::<String>);
    let back: Feedback = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let public_url = Signal::derive(move || ctx.setup.with(|e| e.webhooks.public_url.clone()));
    let source_name = if id == "github" {
        t!(SetupStepGithub)
    } else {
        t!(SetupStepJira)
    };
    let target = Signal::derive(move || {
        webhook_target(public_url.get().as_deref(), &info.with(|i| i.path.clone()))
    });

    let generate = move |_| {
        busy.set(true);
        back.set(None);
        spawn_local(async move {
            match server::post_webhook_secret(id).await {
                Ok(a) => {
                    fresh.set(Some(a.secret));
                    ctx.reload();
                }
                Err(e) => back.set(Some((false, error_text(&e)))),
            }
            busy.set(false);
        });
    };
    let test = move |_| {
        busy.set(true);
        back.set(None);
        spawn_local(async move {
            match server::post_webhook_test(id).await {
                Ok(a) => back.set(Some((a.ok, a.message))),
                Err(e) => back.set(Some((false, error_text(&e)))),
            }
            ctx.reload();
            busy.set(false);
        });
    };

    view! {
        <Section title=title>
            <ol class="ui-instructions">
                <li>
                    {t!(SetupLiveSourceSecretBefore)} " " {source_name} " " {t!(SetupLiveSourceSecretAfter)}
                    <HStack spacing="12px" css="margin-top: 8px; flex-wrap: wrap">
                        <Button
                            variant=ButtonStyle::GlassProminent
                            disabled=Signal::derive(move || busy.get())
                            on_click=generate
                        >
                            {move || {
                                if info.with(|i| i.secret_set) {
                                    t!(SetupLiveSecretRegenerate)
                                } else {
                                    t!(SetupLiveSecretGenerate)
                                }
                            }}
                        </Button>
                        <Status tone=Signal::derive(move || {
                            if info.with(|i| i.secret_set) { StatusTone::Green } else { StatusTone::Gray }
                        })>
                            {move || {
                                if info.with(|i| i.secret_set) {
                                    t!(SetupLiveSecretPresent)
                                } else {
                                    t!(SetupLiveSecretMissing)
                                }
                            }}
                        </Status>
                    </HStack>
                    {move || {
                        fresh
                            .get()
                            .map(|s| {
                                view! {
                                    <div style="margin-top: 8px">
                                        <CopyField store=ctx.store label=t!(SetupLiveSecretLabel) text=Signal::derive(move || s.clone()) />
                                        <p class="ui-hint" role="status">
                                            {t!(SetupLiveSecretNote)}
                                        </p>
                                        <Button size=ControlSize::Small on_click=move |_| fresh.set(None)>
                                            {t!(SetupLiveHide)}
                                        </Button>
                                    </div>
                                }
                            })
                    }}
                </li>
                {children()}
                <li>
                    {t!(SetupLiveTargetIntro)}
                    <CopyField store=ctx.store label=t!(SetupLiveAddress) text=target />
                </li>
            </ol>
            <HStack spacing="12px" css="flex-wrap:wrap">
                <Button disabled=Signal::derive(move || busy.get()) on_click=test>
                    {t!(SetupLiveTestLocal)}
                </Button>
                <Button size=ControlSize::Small on_click=move |_| ctx.reload()>
                    {t!(SetupLiveRefresh)}
                </Button>
            </HStack>
            <Message feedback=back />
            <p class="t-subheadline" role="status">
                {move || {
                    info.with(|i| {
                        webhook_received_text(i.last_received_ms, i.last_event.as_deref(), now_ms())
                            .render(lang())
                    })
                }}
            </p>
        </Section>
    }
}

#[component]
pub fn LiveStep(ctx: Context) -> impl IntoView {
    let enabled = RwSignal::new(false);
    let public_url = RwSignal::new(String::new());
    // Take over the stored values once they have arrived (and again after saving) – but not
    // whenever the status is reloaded, which would undo what the person has just typed.
    let synced = RwSignal::new(false);
    Effect::new(move |_| {
        let (loaded, w) = ctx
            .setup
            .with(|e| (!e.callback_github.is_empty(), e.webhooks.clone()));
        if !loaded || synced.get_untracked() {
            return;
        }
        enabled.set(w.enabled);
        public_url.set(w.public_url.unwrap_or_default());
        synced.set(true);
    });
    let back: Feedback = RwSignal::new(None);
    let save = move |_| match public_url_read(&public_url.get_untracked()) {
        Ok(url) => config_save(
            ctx,
            ConfigChange {
                webhooks: Some(WebhooksInput {
                    enabled: Some(enabled.get_untracked()),
                    public_url: Some(url),
                }),
                ..Default::default()
            },
            back,
            t!(SetupLiveSaved),
            move || synced.set(false),
        ),
        Err(m) => back.set(Some((false, m.render(lang_untracked())))),
    };
    let github = Signal::derive(move || ctx.setup.with(|e| e.webhooks.github.clone()));
    let jira = Signal::derive(move || ctx.setup.with(|e| e.webhooks.jira.clone()));
    let fallback_min =
        Signal::derive(move || ctx.setup.with(|e| e.webhooks.fallback_interval_s / 60));
    let port = port_text();
    let cloudflared = format!("cloudflared tunnel --url http://127.0.0.1:{port}");
    let ngrok = command(&["ngrok", "http", &format!("127.0.0.1:{port}")]);

    view! {
        <VStack spacing="12px">
            <Intro title=t!(SetupLiveTitle)>
                <p>
                    {t!(SetupLiveIntro1)}
                </p>
                <p>
                    {t!(SetupLiveIntro2)}
                </p>
                <p>
                    {t!(SetupLiveIntro3)}
                </p>
            </Intro>
            <Section title=t!(SetupLiveStep1Title)>
                <List>
                    <ListRow title=t!(SetupLiveEnable) toggle_row=true wrap=true>
                        <Toggle checked=enabled aria_label=t!(SetupLiveEnable) />
                    </ListRow>
                </List>
                <TextField
                    label=t!(SetupLivePublicUrlLabel)
                    value=public_url
                    placeholder="https://cockpit.example"
                    hint=t!(SetupLivePublicUrlHint)
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=save>
                        {t!(SetupLiveSave)}
                    </Button>
                </HStack>
                <Message feedback=back />
                <p class="t-footnote t-secondary">
                    {move || {
                        t!(SetupLiveFallback, min = fallback_min.get().max(1))
                    }}
                </p>
            </Section>
            <Section title=t!(SetupLiveStep2Title)>
                <p class="t-subheadline t-secondary">
                    {t!(SetupLiveTunnelIntro)}
                </p>
                <ul class="ui-instructions">
                    <li>
                        <Extern href="https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/">"cloudflared"</Extern>
                        " " {t!(SetupLiveCloudflaredNote)}
                        <CopyField store=ctx.store label=t!(SetupLiveCommand) text=Signal::derive(move || cloudflared.clone()) />
                    </li>
                    <li>
                        <Extern href="https://ngrok.com/docs/getting-started/">"ngrok"</Extern>
                        " " {t!(SetupLiveNgrokNote)}
                        <CopyField store=ctx.store label=t!(SetupLiveCommand) text=Signal::derive(move || ngrok.clone()) />
                    </li>
                </ul>
                <p class="t-subheadline t-secondary">
                    {t!(SetupLiveTunnelAddress)}
                </p>
                <p class="t-subheadline">
                    <strong>{t!(SetupLiveSecurityLabel)}</strong> " "
                    {t!(SetupLiveSecurity)}
                </p>
            </Section>
            <SourceSection ctx=ctx id="github" title=t!(SetupLiveGithubTitle) info=github>
                <li>
                    {t!(SetupLiveGithubOpen)}
                </li>
                <li>
                    {t!(SetupLiveGithubContentType)} " " <code>"application/json"</code> ". " {t!(SetupLiveGithubSecret)}
                </li>
                <li>
                    {t!(SetupLiveGithubEvents)}
                    <ul class="ui-checklist">
                        {GITHUB_WEBHOOK_EVENTS
                            .iter()
                            .map(|e| view! { <li>{*e}</li> })
                            .collect_view()}
                    </ul>
                </li>
            </SourceSection>
            <SourceSection ctx=ctx id="jira" title=t!(SetupLiveJiraTitle) info=jira>
                <li>
                    {t!(SetupLiveJiraOpen)}
                </li>
                <li>
                    {t!(SetupLiveJiraSecretBefore)} <code>"?token=\u{2026}"</code> " " {t!(SetupLiveJiraSecretAfter)}
                </li>
                <li>
                    {t!(SetupLiveJiraEvents)}
                    <ul class="ui-checklist">
                        {JIRA_WEBHOOK_EVENTS
                            .iter()
                            .map(|e| view! { <li>{*e}</li> })
                            .collect_view()}
                    </ul>
                </li>
            </SourceSection>
            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                {t!(SetupLiveFootnote)}
            </p>
        </VStack>
    }
}
