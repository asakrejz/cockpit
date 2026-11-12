//! Schritt 2: Jira verbinden – per Atlassian-App (OAuth) oder, einfacher, per API-Token.

use cockpit_core::api::{ClientEingabe, JiraEingabe, KonfigAenderung};
use cockpit_core::einrichten::{client_id_lesen, projekte_lesen};
use feature_ui::*;
use leptos::prelude::*;

use super::felder::{
    Abschnitt, Einleitung, Extern, Kontext, Kopierfeld, Meldung, Rueckmeldung, SchluesselFeld,
    konfig_speichern,
};
use crate::oauth_card::{OAuthCard, Service};

const WEGE: &[(&str, &str)] = &[
    ("oauth", "Mit Atlassian-App (empfohlen)"),
    ("token", "Mit API-Token (einfacher)"),
];

/// Füllt ein Feld aus dem Server, solange es noch leer ist (der Nutzer tippt nie dagegen an).
fn vorbelegen(feld: RwSignal<String>, quelle: impl Fn() -> String + 'static) {
    Effect::new(move |_| {
        let neu = quelle();
        if feld.get_untracked().is_empty() && !neu.is_empty() {
            feld.set(neu);
        }
    });
}

#[component]
pub fn JiraSchritt(ctx: Kontext) -> impl IntoView {
    let weg = RwSignal::new("oauth");
    Effect::new(move |_| {
        // Wer schon per Token eingerichtet ist, sieht gleich diesen Weg.
        let e = ctx.einr.get();
        if e.atlassian_client_id.is_none() && !e.jira_site.is_empty() {
            weg.set("token");
        }
    });
    view! {
        <VStack spacing="12px">
            <Einleitung title="Jira verbinden">
                <p>
                    "Damit zeigt dir Cockpit deine Tickets, kann sie kommentieren und ihren Status ändern. Es gibt zwei Wege – beide kannst du später wechseln."
                </p>
                <p>
                    <strong>"Atlassian-App:"</strong>
                    " Du legst einmal eine eigene kleine „App“ in deinem Atlassian-Konto an. Cockpit meldet dich damit an, ohne dass ein Passwort weitergegeben wird, und du kannst den Zugriff jederzeit entziehen. Etwas mehr Klickarbeit, dafür sauberer."
                </p>
                <p>
                    <strong>"API-Token:"</strong>
                    " Ein Token ist ein Passwort nur für Programme. Schnell erledigt, gilt aber mit all deinen Rechten, bis du es löschst."
                </p>
            </Einleitung>
            <Segmented options=WEGE value=weg aria_label="Weg der Anmeldung bei Jira" />
            {move || {
                if weg.get() == "oauth" {
                    view! { <JiraOauth ctx=ctx /> }.into_any()
                } else {
                    view! { <JiraToken ctx=ctx /> }.into_any()
                }
            }}
        </VStack>
    }
}

#[component]
fn JiraOauth(ctx: Kontext) -> impl IntoView {
    let client_id = RwSignal::new(String::new());
    vorbelegen(client_id, move || {
        ctx.einr
            .with(|e| e.atlassian_client_id.clone().unwrap_or_default())
    });
    let projekte = RwSignal::new(String::new());
    vorbelegen(projekte, move || {
        ctx.einr.with(|e| e.jira_projekte.join(", "))
    });
    let rueck_id: Rueckmeldung = RwSignal::new(None);
    let rueck_projekte: Rueckmeldung = RwSignal::new(None);
    let callback = Signal::derive(move || ctx.einr.with(|e| e.callback_atlassian.clone()));

    let id_speichern = move |_| match client_id_lesen(&client_id.get_untracked()) {
        Ok(id) if id.is_empty() => rueck_id.set(Some((
            false,
            "Trage die Client-ID aus der Atlassian-Konsole ein.".into(),
        ))),
        Ok(id) => konfig_speichern(
            ctx,
            KonfigAenderung {
                atlassian: Some(ClientEingabe { client_id: id }),
                ..Default::default()
            },
            rueck_id,
            "Client-ID gespeichert.",
            || {},
        ),
        Err(m) => rueck_id.set(Some((false, m))),
    };
    let projekte_speichern = move |_| {
        konfig_speichern(
            ctx,
            KonfigAenderung {
                jira: Some(JiraEingabe {
                    projects: Some(projekte_lesen(&projekte.get_untracked())),
                    ..Default::default()
                }),
                ..Default::default()
            },
            rueck_projekte,
            "Projekte gespeichert.",
            || {},
        )
    };

    view! {
        <Abschnitt title="1. App in der Atlassian-Entwicklerkonsole anlegen">
            <ol class="ui-anleitung">
                <li>
                    "Öffne "
                    <Extern href="https://developer.atlassian.com/console/myapps/">
                        "developer.atlassian.com/console/myapps"
                    </Extern>
                    " und melde dich mit deinem Atlassian-Konto an (dem, mit dem du auch Jira nutzt)."
                </li>
                <li>
                    "Klicke auf „Create“ und wähle „OAuth 2.0 integration“. Gib einen Namen ein, zum Beispiel „Cockpit lokal“, setze den Haken bei den Bedingungen und klicke auf „Create“."
                </li>
                <li>
                    "Wähle links „Permissions“. Falls die Konsole fragt, auf welcher Ebene die Rechte gelten sollen (Resource-level), nimm die Ebene der Ressource, nicht „User identity“. Klicke bei „Jira API“ auf „Add“ und danach auf „Configure“."
                </li>
                <li>
                    "Öffne bei „Classic scopes“ „Edit Scopes“, hake diese drei Rechte an und speichere:"
                    <ul class="ui-haken-liste">
                        <li><code>"read:jira-work"</code><span class="ui-haken-liste__recht">"Tickets lesen"</span></li>
                        <li><code>"write:jira-work"</code><span class="ui-haken-liste__recht">"Tickets kommentieren und Status ändern"</span></li>
                        <li><code>"read:jira-user"</code><span class="ui-haken-liste__recht">"Namen von Personen lesen"</span></li>
                    </ul>
                </li>
                <li>
                    "Wähle links „Authorization“, bei „OAuth 2.0 (3LO)“ „Add“ (oder „Configure“) und trage als „Callback URL“ diese Adresse ein. Dann „Save changes“:"
                    <Kopierfeld store=ctx.store label="Callback-URL" text=callback />
                </li>
                <li>
                    "Wähle links „Settings“. Dort stehen die „Client ID“ und das „Secret“. Beide trägst du gleich unten ein."
                </li>
            </ol>
        </Abschnitt>
        <Abschnitt title="2. Client-ID und Client-Secret eintragen">
            <TextField
                label="Client ID"
                value=client_id
                hint="Nicht geheim. Kopiere sie genau so, wie sie in der Konsole steht."
                autocomplete="off"
            />
            <HStack spacing="12px">
                <Button variant=ButtonStyle::GlassProminent on_click=id_speichern>
                    "Client-ID speichern"
                </Button>
            </HStack>
            <Meldung rueckmeldung=rueck_id />
            <SchluesselFeld
                ctx=ctx
                name="atlassian-oauth"
                label="Client-Secret (Secret)"
                hinweis="Geheim wie ein Passwort. Es wird nur im Schlüsselbund dieses Rechners gespeichert und nie wieder angezeigt."
            />
        </Abschnitt>
        <Abschnitt title="3. Welche Projekte? (optional)">
            <TextField
                label="Jira-Projekte"
                value=projekte
                hint="Kürzel mit Komma getrennt, z. B. BAU, WEB. Leer lassen: Cockpit zeigt dann deine Tickets und die, die du beobachtest. In der Inbox kannst du später ein Projekt wählen."
                autocomplete="off"
            />
            <HStack spacing="12px">
                <Button on_click=projekte_speichern>"Projekte speichern"</Button>
            </HStack>
            <Meldung rueckmeldung=rueck_projekte />
        </Abschnitt>
        <Abschnitt title="4. Anmelden">
            <p class="t-subheadline t-secondary">
                "Es öffnet sich eine Seite von Atlassian. Prüfe dort den Namen deiner Site und bestätige mit „Accept“."
            </p>
            <OAuthCard
                service=Service::Atlassian
                on_connected=move |_| ctx.laden()
                on_disconnected=move |_| ctx.laden()
            />
        </Abschnitt>
    }
}

#[component]
fn JiraToken(ctx: Kontext) -> impl IntoView {
    let site = RwSignal::new(String::new());
    vorbelegen(site, move || ctx.einr.with(|e| e.jira_site.clone()));
    let email = RwSignal::new(String::new());
    vorbelegen(email, move || ctx.einr.with(|e| e.jira_email.clone()));
    let rueck: Rueckmeldung = RwSignal::new(None);

    let speichern = move |_| {
        konfig_speichern(
            ctx,
            KonfigAenderung {
                jira: Some(JiraEingabe {
                    site: Some(site.get_untracked()),
                    email: Some(email.get_untracked()),
                    projects: None,
                }),
                ..Default::default()
            },
            rueck,
            "Site und E-Mail gespeichert.",
            || {},
        )
    };
    let verbunden = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().is_some_and(|h| h.anmeldung.jira.verbunden))
    };

    view! {
        <Abschnitt title="1. Token in deinem Atlassian-Konto erzeugen">
            <ol class="ui-anleitung">
                <li>
                    "Öffne "
                    <Extern href="https://id.atlassian.com/manage-profile/security/api-tokens">
                        "id.atlassian.com/manage-profile/security/api-tokens"
                    </Extern>
                    " und melde dich an."
                </li>
                <li>
                    "Klicke auf „Create API token“, gib einen Namen wie „Cockpit“ ein, wähle ein Ablaufdatum und klicke auf „Create“."
                </li>
                <li>
                    "Kopiere das Token mit „Copy“. Es wird nur jetzt angezeigt – füge es gleich unten ein."
                </li>
            </ol>
        </Abschnitt>
        <Abschnitt title="2. Angaben eintragen">
            <TextField
                label="Jira-Adresse (Site)"
                value=site
                placeholder="firma.atlassian.net"
                hint="So beginnt die Adresse, wenn du Jira im Browser öffnest. „firma“ oder „firma.atlassian.net“ genügt."
                autocomplete="off"
            />
            <TextField
                label="E-Mail-Adresse deines Atlassian-Kontos"
                value=email
                input_type="email"
                autocomplete="off"
            />
            <HStack spacing="12px">
                <Button variant=ButtonStyle::GlassProminent on_click=speichern>
                    "Site und E-Mail speichern"
                </Button>
            </HStack>
            <Meldung rueckmeldung=rueck />
            <SchluesselFeld
                ctx=ctx
                name="jira"
                label="API-Token"
                hinweis="Geheim wie ein Passwort. Es wird nur im Schlüsselbund dieses Rechners gespeichert."
            />
            <Status tone=Signal::derive(move || {
                if verbunden() { StatusTone::Green } else { StatusTone::Gray }
            })>
                {move || {
                    if verbunden() {
                        "Verbunden"
                    } else {
                        "Noch nicht verbunden – speichere Site, E-Mail und Token."
                    }
                }}
            </Status>
        </Abschnitt>
    }
}
