//! Step 3: connect GitHub – via an own GitHub App.

use cockpit_core::api::{ClientInput, ConfigChange, GitHubInput};
use cockpit_core::setup::{client_id_read, repos_read};
use feature_ui::*;
use leptos::prelude::*;

use super::fields::{
    Context, CopyField, Extern, Feedback, Intro, KeyField, Message, Section, config_save,
};
use crate::oauth_card::{OAuthCard, Service};

/// Permissions of the GitHub App: (area, level).
const PERMISSIONS: &[(&str, &str)] = &[
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
pub fn GitHubStep(ctx: Context) -> impl IntoView {
    let client_id = RwSignal::new(String::new());
    Effect::new(move |_| {
        let updated = ctx
            .setup
            .with(|e| e.github_client_id.clone().unwrap_or_default());
        if client_id.get_untracked().is_empty() && !updated.is_empty() {
            client_id.set(updated);
        }
    });
    let repos = RwSignal::new(String::new());
    Effect::new(move |_| {
        let updated = ctx.setup.with(|e| e.github_repos.join(", "));
        if repos.get_untracked().is_empty() && !updated.is_empty() {
            repos.set(updated);
        }
    });
    let back_id: Feedback = RwSignal::new(None);
    let back_repos: Feedback = RwSignal::new(None);
    let callback = Signal::derive(move || ctx.setup.with(|e| e.callback_github.clone()));

    let id_save = move |_| match client_id_read(&client_id.get_untracked()) {
        Ok(id) if id.is_empty() => back_id.set(Some((
            false,
            "Trage die Client ID von der Seite deiner GitHub App ein.".into(),
        ))),
        Ok(id) => config_save(
            ctx,
            ConfigChange {
                github_oauth: Some(ClientInput { client_id: id }),
                ..Default::default()
            },
            back_id,
            "Client ID gespeichert.",
            || {},
        ),
        Err(m) => back_id.set(Some((false, m))),
    };
    let repos_save = move |_| match repos_read(&repos.get_untracked()) {
        Ok(list) => config_save(
            ctx,
            ConfigChange {
                github: Some(GitHubInput { repos: list }),
                ..Default::default()
            },
            back_repos,
            "Repos gespeichert.",
            || {},
        ),
        Err(m) => back_repos.set(Some((false, m))),
    };

    view! {
        <VStack spacing="12px">
            <Intro title="GitHub verbinden">
                <p>
                    "Damit sieht Cockpit deine Pull Requests und Reviews. Du legst dafür einmal eine eigene „GitHub App“ an – ein kleines Zugangs-Konto, das nur auf die Repos zugreifen darf, die du ihr gibst. Du kannst sie jederzeit löschen."
                </p>
            </Intro>
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
                        "Trage bei „Callback URL“ diese Adresse ein und lass „Expire user authorization tokens“ angehakt:"
                        <CopyField store=ctx.store label="Callback-URL" text=callback />
                    </li>
                    <li>
                        "Nimm bei „Webhook“ den Haken bei „Active“ heraus – Webhooks braucht Cockpit noch nicht."
                    </li>
                    <li>
                        "Öffne unter „Permissions“ den Bereich „Repository permissions“ und stelle diese Rechte ein (Haken setzen, was du schon erledigt hast):"
                        <ul class="ui-checklist">
                            {PERMISSIONS
                                .iter()
                                .map(|&(b, s)| view! { <Checkmark area=b level=s /> })
                                .collect_view()}
                        </ul>
                    </li>
                    <li>
                        "Bei „Where can this GitHub App be installed?“ wähle „Only on this account“. Klicke dann unten auf „Create GitHub App“."
                    </li>
                </ol>
            </Section>
            <Section title="2. Client ID und Client-Secret eintragen">
                <ol class="ui-instructions">
                    <li>
                        "Oben auf der Seite deiner neuen App steht die „Client ID“ (sie beginnt oft mit „Iv“). Trage sie hier ein."
                    </li>
                    <li>
                        "Weiter unten klickst du auf „Generate a new client secret“ und kopierst es sofort – GitHub zeigt es nur einmal. Füge es unten ein."
                    </li>
                </ol>
                <TextField label="Client ID" value=client_id hint="Nicht geheim." autocomplete="off" />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=id_save>
                        "Client ID speichern"
                    </Button>
                </HStack>
                <Message feedback=back_id />
                <KeyField
                    ctx=ctx
                    name="github-oauth"
                    label="Client-Secret"
                    hint="Geheim wie ein Passwort. Es wird nur in der .env-Datei im Projektordner dieses Rechners gespeichert und nie wieder angezeigt."
                />
            </Section>
            <Section title="3. App installieren – sonst sieht Cockpit keine Repos">
                <ol class="ui-instructions">
                    <li>"Klicke links auf der Seite deiner App auf „Install App“ und dann bei deinem Konto auf „Install“."</li>
                    <li>
                        "Wähle „Only select repositories“ und hake die Repos an, die Cockpit sehen soll. Dann „Install“."
                    </li>
                </ol>
            </Section>
            <Section title="4. Welche Repos gleicht Cockpit ab?">
                <TextField
                    label="Repos"
                    value=repos
                    placeholder="firma/app, firma/web"
                    hint="Form „besitzer/name“, mit Komma getrennt. Du kannst auch die Adresse aus der Browserzeile einfügen."
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button on_click=repos_save>"Repos speichern"</Button>
                </HStack>
                <Message feedback=back_repos />
            </Section>
            <Section title="5. Anmelden">
                <p class="t-subheadline t-secondary">
                    "Es öffnet sich eine Seite von GitHub. Bestätige dort mit „Authorize“."
                </p>
                <OAuthCard
                    service=Service::GitHub
                    on_connected=move |_| ctx.reload()
                    on_disconnected=move |_| ctx.reload()
                />
            </Section>
        </VStack>
    }
}
