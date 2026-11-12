//! Schritt 3: GitHub verbinden – über eine eigene GitHub App.

use cockpit_core::api::{ClientEingabe, GitHubEingabe, KonfigAenderung};
use cockpit_core::einrichten::{client_id_lesen, repos_lesen};
use feature_ui::*;
use leptos::prelude::*;

use super::felder::{
    Abschnitt, Einleitung, Extern, Kontext, Kopierfeld, Meldung, Rueckmeldung, SchluesselFeld,
    konfig_speichern,
};
use crate::oauth_card::{OAuthCard, Service};

/// Berechtigungen der GitHub App: (Bereich, Stufe).
const RECHTE: &[(&str, &str)] = &[
    ("Contents", "Read & write"),
    ("Pull requests", "Read & write"),
    ("Issues", "Read & write"),
    ("Checks", "Read & write"),
    ("Commit statuses", "Read-only"),
    ("Metadata", "Read-only (schon vorausgewählt)"),
];

/// Ein Eintrag zum Abhaken. Der Haken lebt nur auf der Seite, er wird nicht gespeichert.
#[component]
fn Haken(bereich: &'static str, stufe: &'static str) -> impl IntoView {
    let an = RwSignal::new(false);
    view! {
        <li>
            <label style="display: inline-flex; gap: 8px; align-items: baseline; cursor: pointer">
                <input type="checkbox" prop:checked=move || an.get() on:change=move |_| an.update(|a| *a = !*a) />
                <span style:text-decoration=move || an.get().then_some("line-through")>
                    <strong>{bereich}</strong>
                </span>
            </label>
            <span class="ui-haken-liste__recht">{stufe}</span>
        </li>
    }
}

#[component]
pub fn GitHubSchritt(ctx: Kontext) -> impl IntoView {
    let client_id = RwSignal::new(String::new());
    Effect::new(move |_| {
        let neu = ctx
            .einr
            .with(|e| e.github_client_id.clone().unwrap_or_default());
        if client_id.get_untracked().is_empty() && !neu.is_empty() {
            client_id.set(neu);
        }
    });
    let repos = RwSignal::new(String::new());
    Effect::new(move |_| {
        let neu = ctx.einr.with(|e| e.github_repos.join(", "));
        if repos.get_untracked().is_empty() && !neu.is_empty() {
            repos.set(neu);
        }
    });
    let rueck_id: Rueckmeldung = RwSignal::new(None);
    let rueck_repos: Rueckmeldung = RwSignal::new(None);
    let callback = Signal::derive(move || ctx.einr.with(|e| e.callback_github.clone()));

    let id_speichern = move |_| match client_id_lesen(&client_id.get_untracked()) {
        Ok(id) if id.is_empty() => rueck_id.set(Some((
            false,
            "Trage die Client ID von der Seite deiner GitHub App ein.".into(),
        ))),
        Ok(id) => konfig_speichern(
            ctx,
            KonfigAenderung {
                github_oauth: Some(ClientEingabe { client_id: id }),
                ..Default::default()
            },
            rueck_id,
            "Client ID gespeichert.",
            || {},
        ),
        Err(m) => rueck_id.set(Some((false, m))),
    };
    let repos_speichern = move |_| match repos_lesen(&repos.get_untracked()) {
        Ok(liste) => konfig_speichern(
            ctx,
            KonfigAenderung {
                github: Some(GitHubEingabe { repos: liste }),
                ..Default::default()
            },
            rueck_repos,
            "Repos gespeichert.",
            || {},
        ),
        Err(m) => rueck_repos.set(Some((false, m))),
    };

    view! {
        <VStack spacing="12px">
            <Einleitung title="GitHub verbinden">
                <p>
                    "Damit sieht Cockpit deine Pull Requests und Reviews. Du legst dafür einmal eine eigene „GitHub App“ an – ein kleines Zugangs-Konto, das nur auf die Repos zugreifen darf, die du ihr gibst. Du kannst sie jederzeit löschen."
                </p>
            </Einleitung>
            <Abschnitt title="1. GitHub App anlegen">
                <ol class="ui-anleitung">
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
                        <Kopierfeld store=ctx.store label="Callback-URL" text=callback />
                    </li>
                    <li>
                        "Nimm bei „Webhook“ den Haken bei „Active“ heraus – Webhooks braucht Cockpit noch nicht."
                    </li>
                    <li>
                        "Öffne unter „Permissions“ den Bereich „Repository permissions“ und stelle diese Rechte ein (Haken setzen, was du schon erledigt hast):"
                        <ul class="ui-haken-liste">
                            {RECHTE
                                .iter()
                                .map(|&(b, s)| view! { <Haken bereich=b stufe=s /> })
                                .collect_view()}
                        </ul>
                    </li>
                    <li>
                        "Bei „Where can this GitHub App be installed?“ wähle „Only on this account“. Klicke dann unten auf „Create GitHub App“."
                    </li>
                </ol>
            </Abschnitt>
            <Abschnitt title="2. Client ID und Client-Secret eintragen">
                <ol class="ui-anleitung">
                    <li>
                        "Oben auf der Seite deiner neuen App steht die „Client ID“ (sie beginnt oft mit „Iv“). Trage sie hier ein."
                    </li>
                    <li>
                        "Weiter unten klickst du auf „Generate a new client secret“ und kopierst es sofort – GitHub zeigt es nur einmal. Füge es unten ein."
                    </li>
                </ol>
                <TextField label="Client ID" value=client_id hint="Nicht geheim." autocomplete="off" />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=id_speichern>
                        "Client ID speichern"
                    </Button>
                </HStack>
                <Meldung rueckmeldung=rueck_id />
                <SchluesselFeld
                    ctx=ctx
                    name="github-oauth"
                    label="Client-Secret"
                    hinweis="Geheim wie ein Passwort. Es wird nur im Schlüsselbund dieses Rechners gespeichert und nie wieder angezeigt."
                />
            </Abschnitt>
            <Abschnitt title="3. App installieren – sonst sieht Cockpit keine Repos">
                <ol class="ui-anleitung">
                    <li>"Klicke links auf der Seite deiner App auf „Install App“ und dann bei deinem Konto auf „Install“."</li>
                    <li>
                        "Wähle „Only select repositories“ und hake die Repos an, die Cockpit sehen soll. Dann „Install“."
                    </li>
                </ol>
            </Abschnitt>
            <Abschnitt title="4. Welche Repos gleicht Cockpit ab?">
                <TextField
                    label="Repos"
                    value=repos
                    placeholder="firma/app, firma/web"
                    hint="Form „besitzer/name“, mit Komma getrennt. Du kannst auch die Adresse aus der Browserzeile einfügen."
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button on_click=repos_speichern>"Repos speichern"</Button>
                </HStack>
                <Meldung rueckmeldung=rueck_repos />
            </Abschnitt>
            <Abschnitt title="5. Anmelden">
                <p class="t-subheadline t-secondary">
                    "Es öffnet sich eine Seite von GitHub. Bestätige dort mit „Authorize“."
                </p>
                <OAuthCard
                    service=Service::GitHub
                    on_connected=move |_| ctx.laden()
                    on_disconnected=move |_| ctx.laden()
                />
            </Abschnitt>
        </VStack>
    }
}
