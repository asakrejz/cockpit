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
    Context, CopyField, Extern, Feedback, Intro, Message, Section, config_save, error_text,
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
    title: &'static str,
    #[prop(into)] info: Signal<WebhookSourceInfo>,
    children: Children,
) -> impl IntoView {
    // The secret just generated: lives only in this signal, never stored, gone when the page is left.
    let fresh = RwSignal::new(None::<String>);
    let back: Feedback = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let public_url = Signal::derive(move || ctx.setup.with(|e| e.webhooks.public_url.clone()));
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
                    "Erzeuge ein Secret. Das ist ein langes Zufalls-Passwort, mit dem "
                    {title}
                    " seine Nachrichten unterschreibt. Cockpit prüft die Unterschrift und nimmt sonst nichts an."
                    <HStack spacing="12px" css="margin-top: 8px; flex-wrap: wrap">
                        <Button
                            variant=ButtonStyle::GlassProminent
                            disabled=Signal::derive(move || busy.get())
                            on_click=generate
                        >
                            {move || {
                                if info.with(|i| i.secret_set) {
                                    "Neues Secret erzeugen (ersetzt das alte)"
                                } else {
                                    "Secret erzeugen"
                                }
                            }}
                        </Button>
                        <Status tone=Signal::derive(move || {
                            if info.with(|i| i.secret_set) { StatusTone::Green } else { StatusTone::Gray }
                        })>
                            {move || {
                                if info.with(|i| i.secret_set) {
                                    "Liegt in der .env-Datei"
                                } else {
                                    "Noch kein Secret"
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
                                        <CopyField store=ctx.store label="Secret (nur jetzt sichtbar)" text=Signal::derive(move || s.clone()) />
                                        <p class="ui-hint" role="status">
                                            "Kopiere es jetzt und füge es unten ein. Cockpit zeigt es nie wieder an; es liegt nur in der .env-Datei."
                                        </p>
                                        <Button size=ControlSize::Small on_click=move |_| fresh.set(None)>
                                            "Ausblenden"
                                        </Button>
                                    </div>
                                }
                            })
                    }}
                </li>
                {children()}
                <li>
                    "Ziel-Adresse (Payload URL) für den Webhook:"
                    <CopyField store=ctx.store label="Adresse" text=target />
                </li>
            </ol>
            <HStack spacing="12px" css="flex-wrap: wrap">
                <Button disabled=Signal::derive(move || busy.get()) on_click=test>
                    "Lokal testen"
                </Button>
                <Button size=ControlSize::Small on_click=move |_| ctx.reload()>
                    "Aktualisieren"
                </Button>
            </HStack>
            <Message feedback=back />
            <p class="t-subheadline" role="status">
                {move || {
                    info.with(|i| {
                        webhook_received_text(i.last_received_ms, i.last_event.as_deref(), now_ms())
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
            "Gespeichert.",
            move || synced.set(false),
        ),
        Err(m) => back.set(Some((false, m))),
    };
    let github = Signal::derive(move || ctx.setup.with(|e| e.webhooks.github.clone()));
    let jira = Signal::derive(move || ctx.setup.with(|e| e.webhooks.jira.clone()));
    let fallback_min =
        Signal::derive(move || ctx.setup.with(|e| e.webhooks.fallback_interval_s / 60));
    let port = port_text();
    let cloudflared = format!("cloudflared tunnel --url http://127.0.0.1:{port}");
    let ngrok = format!("ngrok http 127.0.0.1:{port}");

    view! {
        <VStack spacing="12px">
            <Intro title="Live-Update (optional)">
                <p>
                    "Standardmäßig fragt Cockpit Jira und GitHub alle paar Minuten ab. Das funktioniert ohne jede Einrichtung und bleibt so. Mit Webhooks melden sich GitHub und Jira von selbst, sobald sich etwas ändert, und Cockpit gleicht sofort ab."
                </p>
                <p>
                    "Dafür müssen GitHub und Jira dein Cockpit aus dem Internet erreichen können. Das geht nur über einen Tunnel, den du selbst startest. Cockpit installiert und startet nichts davon. Wenn du das nicht brauchst: überspringen."
                </p>
                <p>
                    "Ein Webhook ist nur ein Signal. Cockpit übernimmt nichts aus der Nachricht, sondern holt die Daten wie immer über die angemeldete Verbindung."
                </p>
            </Intro>
            <Section title="1. Webhooks einschalten">
                <List>
                    <ListRow title="Webhooks aktivieren" toggle_row=true wrap=true>
                        <Toggle checked=enabled aria_label="Webhooks aktivieren" />
                    </ListRow>
                </List>
                <TextField
                    label="Öffentliche Adresse des Tunnels"
                    value=public_url
                    placeholder="https://cockpit.example"
                    hint="Nur zur Anzeige: Cockpit baut daraus die Adressen zum Kopieren und ruft sie nie selbst auf. Du bekommst sie von deinem Tunnel (Schritt 2). Muss mit https:// beginnen."
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=save>
                        "Speichern"
                    </Button>
                </HStack>
                <Message feedback=back />
                <p class="t-footnote t-secondary">
                    {move || {
                        format!(
                            "Solange Webhooks regelmäßig ankommen, fragt Cockpit die jeweilige Quelle nur noch alle {} Minuten als Sicherheitsnetz ab; bleiben sie aus, gilt wieder das normale Intervall.",
                            fallback_min.get().max(1)
                        )
                    }}
                </p>
            </Section>
            <Section title="2. Tunnel starten (du, nicht Cockpit)">
                <p class="t-subheadline t-secondary">
                    "Ein Tunnel gibt deinem Rechner eine öffentliche https-Adresse. Wähle eines und starte es in einem Terminal, das offen bleibt:"
                </p>
                <ul class="ui-instructions">
                    <li>
                        <Extern href="https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/">"cloudflared"</Extern>
                        " (Cloudflare, ohne Konto für einen Kurztunnel):"
                        <CopyField store=ctx.store label="Befehl" text=Signal::derive(move || cloudflared.clone()) />
                    </li>
                    <li>
                        <Extern href="https://ngrok.com/docs/getting-started/">"ngrok"</Extern>
                        " (Konto nötig):"
                        <CopyField store=ctx.store label="Befehl" text=Signal::derive(move || ngrok.clone()) />
                    </li>
                </ul>
                <p class="t-subheadline t-secondary">
                    "Der Tunnel zeigt dir eine Adresse mit https://. Trage sie oben als „Öffentliche Adresse“ ein. Kurztunnel bekommen bei jedem Start eine neue Adresse; dann musst du sie auch bei GitHub und Jira ändern."
                </p>
                <p class="t-subheadline">
                    <strong>"Sicherheit: "</strong>
                    "Nur die zwei Adressen /api/webhooks/github und /api/webhooks/jira sind für das Internet gedacht. Alles andere in Cockpit lehnt Anfragen mit fremdem Adressnamen ab. Besser ist trotzdem, wenn dein Tunnel nur Pfade unter /api/webhooks/ durchlässt (bei Cloudflare z. B. über eine Regel für den Pfad). Ohne Secret nimmt Cockpit nichts an."
                </p>
            </Section>
            <SourceSection ctx=ctx id="github" title="3. GitHub-Webhook" info=github>
                <li>
                    "Öffne bei GitHub die Einstellungen deines Repos (Settings › Webhooks › Add webhook) oder, wenn du eine GitHub App nutzt, die Seite der App (Settings › Developer settings › GitHub Apps › deine App › General › Webhook, „Active“ anhaken)."
                </li>
                <li>
                    "Content type: " <code>"application/json"</code> ". Füge das Secret bei „Secret“ ein."
                </li>
                <li>
                    "Ereignisse („Let me select individual events“):"
                    <ul class="ui-checklist">
                        {GITHUB_WEBHOOK_EVENTS
                            .iter()
                            .map(|e| view! { <li>{*e}</li> })
                            .collect_view()}
                    </ul>
                </li>
            </SourceSection>
            <SourceSection ctx=ctx id="jira" title="4. Jira-Webhook" info=jira>
                <li>
                    "Öffne in Jira (als Administrator) Einstellungen › System › WebHooks › „Create a WebHook“."
                </li>
                <li>
                    "Füge das Secret bei „Secret“ ein; Jira unterschreibt damit jede Nachricht. Gibt es dieses Feld bei dir nicht (ältere Jira-Versionen, Automatisierungsregeln), hänge das Secret stattdessen als " <code>"?token=…"</code> " an die Adresse. Das ist schwächer: das Secret steht dann in der Adresse und kann in Protokollen auftauchen."
                </li>
                <li>
                    "Ereignisse:"
                    <ul class="ui-checklist">
                        {JIRA_WEBHOOK_EVENTS
                            .iter()
                            .map(|e| view! { <li>{*e}</li> })
                            .collect_view()}
                    </ul>
                </li>
            </SourceSection>
            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                "„Lokal testen“ schickt eine unterschriebene Nachricht von Cockpit an sich selbst und prüft so Secret und Verdrahtung, ohne GitHub oder Jira. Ob dich die Dienste über den Tunnel erreichen, zeigt „Zuletzt empfangen“ erst nach einer echten Zustellung (bei GitHub: „Recent Deliveries“ › „Redeliver“)."
            </p>
        </VStack>
    }
}
