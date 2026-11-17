//! Step 3: connect GitHub. The default is a fine-grained personal access token; the own
//! GitHub App (OAuth) is the second way. Both end in the same list of repos and the same test.

use cockpit_core::api::{ClientInput, ConfigChange, GitHubCheck, RepoState};
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::client_id_read;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::auth_method::{AuthMethodPicker, current};
use super::fields::{
    Context, CopyField, Extern, Feedback, Intro, KeyField, Message, Section, config_save,
    error_text,
};
use super::repo_picker::RepoPicker;
use crate::oauth_card::{OAuthCard, Service};
use crate::server;

/// Where a fine-grained token is created.
const TOKEN_PAGE: &str = "https://github.com/settings/personal-access-tokens/new";

/// Rights of the token: (area, level).
const TOKEN_RIGHTS: &[(&str, &str)] = &[
    ("Contents", "Read and write"),
    ("Pull requests", "Read and write"),
    ("Issues", "Read and write"),
    ("Checks", "Read and write"),
    ("Metadata", "Read-only (ist immer dabei)"),
];

/// Permissions of the GitHub App: (area, level).
const APP_RIGHTS: &[(&str, &str)] = &[
    ("Contents", "Read & write"),
    ("Pull requests", "Read & write"),
    ("Issues", "Read & write"),
    ("Checks", "Read & write"),
    ("Commit statuses", "Read-only"),
    ("Metadata", "Read-only (schon vorausgewählt)"),
];

/// An entry to tick off. The tick only lives on the page, it is not saved.
#[component]
fn Checkmark(area: &'static str, level: &'static str) -> impl IntoView {
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
fn Rights(list: &'static [(&'static str, &'static str)]) -> impl IntoView {
    view! {
        <ul class="ui-checklist">
            {list.iter().map(|&(b, s)| view! { <Checkmark area=b level=s /> }).collect_view()}
        </ul>
    }
}

/// The way with a token: instructions and a password field.
#[component]
fn TokenWay(ctx: Context) -> impl IntoView {
    view! {
        <Section title="1. Token erzeugen">
            <ol class="ui-instructions">
                <li>
                    "Öffne "
                    <Extern href=TOKEN_PAGE>"github.com/settings/personal-access-tokens/new"</Extern>
                    " und melde dich bei Bedarf an. Vergib einen Namen, z. B. „Cockpit“, und ein Ablaufdatum."
                </li>
                <li>
                    "Wähle bei „Repository access“ die Option „Only select repositories“ und nimm nur die Repos, die Cockpit sehen soll – am besten zuerst nur Test-Repos. Gehören die Repos einer Organisation, wähle diese bei „Resource owner“."
                </li>
                <li>
                    "Öffne „Repository permissions“ und stelle diese Rechte ein (Haken setzen, was du erledigt hast):"
                    <Rights list=TOKEN_RIGHTS />
                </li>
                <li>"Klicke auf „Generate token“ und kopiere den Token sofort – GitHub zeigt ihn nur einmal."</li>
            </ol>
        </Section>
        <Section title="2. Token ablegen">
            <KeyField
                ctx=ctx
                name="github"
                label="Token"
                hint="Geheim wie ein Passwort. Er wird nur in der .env-Datei im Projektordner dieses Rechners gespeichert und nie wieder angezeigt."
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
                back.set(Some((
                    false,
                    "Trage die Client ID von der Seite deiner GitHub App ein.".into(),
                )));
                return;
            }
            Ok(id) => id,
            Err(m) => {
                back.set(Some((false, m)));
                return;
            }
        };
        let typed = secret.get_untracked();
        let typed = typed.trim().to_string();
        let to_store = if typed.is_empty() {
            if !secret_set() {
                back.set(Some((
                    false,
                    "Füge auch das Client-Secret ein – GitHub zeigt es nur einmal.".into(),
                )));
                return;
            }
            None
        } else {
            match cockpit_core::setup::key_verify(&typed) {
                Ok(v) => Some(v),
                Err(m) => {
                    back.set(Some((false, m)));
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
                    let mut text = String::from(
                        "Client ID und Client-Secret gespeichert. Das Secret-Feld ist wieder leer.",
                    );
                    if let Some(h) = answer.hint {
                        text.push(' ');
                        text.push_str(&h);
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
            "Client ID entfernt. Das Client-Secret bleibt in der .env-Datei, bis du es überschreibst.",
            || {},
        );
    };

    view! {
        <Section title="1. GitHub App anlegen">
            <ol class="ui-instructions">
                <li>
                    "Öffne "
                    <Extern href="https://github.com/settings/apps/new">"github.com/settings/apps/new"</Extern>
                    " (für ein Organisations-Konto: Einstellungen der Organisation › Developer settings › GitHub Apps › New GitHub App). Melde dich bei Bedarf an."
                </li>
                <li>
                    "Name: zum Beispiel „Cockpit von …“ mit deinem Namen. Der Name muss weltweit einmalig sein. „Homepage URL“: " <code>"https://github.com"</code> " genügt."
                </li>
                <li>
                    "Trage bei „Callback URL“ genau diese Adresse ein (mit 127.0.0.1, nicht „localhost“) und lass „Expire user authorization tokens“ angehakt:"
                    <CopyField store=ctx.store label="Callback-URL" text=callback />
                </li>
                <li>
                    "Lass „Setup URL“ leer. Nimm bei „Webhook“ den Haken bei „Active“ heraus – Webhooks braucht Cockpit hier nicht."
                </li>
                <li>
                    "Öffne unter „Permissions“ den Bereich „Repository permissions“ und stelle diese Rechte ein (Haken setzen, was du schon erledigt hast):"
                    <Rights list=APP_RIGHTS />
                </li>
                <li>
                    "Bei „Where can this GitHub App be installed?“ wähle „Only on this account“. Klicke dann unten auf „Create GitHub App“."
                </li>
            </ol>
        </Section>
        <Section title="2. Client ID und Client-Secret eintragen">
            <ol class="ui-instructions">
                <li>
                    "Oben auf der Seite deiner neuen App steht die „Client ID“ (sie beginnt oft mit „Iv“)."
                </li>
                <li>
                    "Weiter unten klickst du auf „Generate a new client secret“ und kopierst es sofort – GitHub zeigt es nur einmal."
                </li>
                <li>
                    "Einen „Private key“ brauchst du nicht – lass ihn weg. Cockpit braucht nur Client ID und Client-Secret."
                </li>
            </ol>
            <TextField label="Client ID" value=client_id hint="Nicht geheim." autocomplete="off" />
            <TextField
                label="Client-Secret"
                value=secret
                input_type="password"
                autocomplete="off"
                hint="Geheim wie ein Passwort. Es wird nur in der .env-Datei im Projektordner dieses Rechners gespeichert und nie wieder angezeigt. Leer lassen, um nur die Client ID zu ändern."
            />
            <HStack spacing="12px">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=Signal::derive(move || running.get())
                    on_click=save
                >
                    "Speichern"
                </Button>
                <Button
                    variant=ButtonStyle::Borderless
                    disabled=Signal::derive(move || {
                        running.get() || ctx.setup.with(|e| e.github_client_id.is_none())
                    })
                    on_click=remove
                >
                    "Client-ID entfernen"
                </Button>
                <Status tone=Signal::derive(move || {
                    if secret_set() { StatusTone::Green } else { StatusTone::Gray }
                })>
                    {move || if secret_set() { "Client-Secret liegt in der .env-Datei" } else { "Client-Secret fehlt noch" }}
                </Status>
            </HStack>
            <Message feedback=back />
        </Section>
        <Section title="3. App installieren – sonst sieht Cockpit keine Repos">
            <ol class="ui-instructions">
                <li>"Klicke links auf der Seite deiner App auf „Install App“ und dann bei deinem Konto auf „Install“."</li>
                <li>
                    "Wähle „Only select repositories“ und hake die Repos an, die Cockpit sehen soll. Dann „Install“."
                </li>
            </ol>
        </Section>
        <Section title="4. Anmelden">
            <p class="t-subheadline t-secondary">
                "Es öffnet sich eine Seite von GitHub. Bestätige dort mit „Authorize“."
            </p>
            <OAuthCard
                service=Service::GitHub
                on_connected=move |_| ctx.reload()
                on_disconnected=move |_| ctx.reload()
            />
        </Section>
    }
}

/// „Verbindung prüfen“: user and chosen repos, read-only.
#[component]
fn ConnectionCheck(ctx: Context) -> impl IntoView {
    let result: RwSignal<Option<GitHubCheck>> = RwSignal::new(None);
    let feedback: Feedback = RwSignal::new(None);
    let running = RwSignal::new(false);
    let go = move |_| {
        let repos = ctx.setup.with(|s| s.agent_repos.names());
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
        <Section title="Verbindung prüfen">
            <p class="t-subheadline t-secondary">
                "Cockpit fragt GitHub, wer du bist, und schaut bei jedem gespeicherten Repo nach, ob es sichtbar ist und ob du dort schreiben darfst. Es wird nichts verändert."
            </p>
            <HStack spacing="12px">
                <Button on_click=go disabled=Signal::derive(move || running.get())>
                    {move || if running.get() { "Prüfe …" } else { "Verbindung prüfen" }}
                </Button>
            </HStack>
            <Message feedback=feedback />
            {move || {
                result
                    .get()
                    .map(|a| {
                        let tone = a.ok;
                        view! {
                            <p class="ui-hint" role="status" style=(!tone).then_some("color: var(--sys-color-red)")>
                                {a.message.clone()}
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
                                        view! { <li>{format!("{mark} {} – {}", c.repo, c.message)}</li> }
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
    let default = Signal::derive(move || {
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

    view! {
        <VStack spacing="12px">
            <Intro title="GitHub verbinden">
                <p>
                    "Damit sieht Cockpit deine Pull Requests und Reviews, und der Agent kann in deinen Repos arbeiten. Der einfachste Weg ist ein Token, der nur die Repos sieht, die du auswählst. Du kannst ihn jederzeit auf GitHub löschen."
                </p>
            </Intro>
            <AuthMethodPicker chosen=chosen default=default aria_label="Anmeldeweg für GitHub" />
            <div hidden=move || method.get() != AuthKind::Token>
                <VStack spacing="12px">
                    <TokenWay ctx=ctx />
                </VStack>
            </div>
            <div hidden=move || method.get() != AuthKind::OAuth>
                <VStack spacing="12px">
                    <AppWay ctx=ctx />
                </VStack>
            </div>
            <RepoPicker ctx=ctx />
            <ConnectionCheck ctx=ctx />
        </VStack>
    }
}
