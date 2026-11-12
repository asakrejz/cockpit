//! Schritt 2: Jira verbinden – Standard ist der API-Token (drei einfache Schritte, danach ein
//! Verbindungstest und die Wahl des Projekts); die Atlassian-App (OAuth) steckt eingeklappt unter
//! „Erweitert“. Die Regeln (Adresse lesen, Texte) stehen in `cockpit_core::setup`.

use cockpit_core::Action;
use cockpit_core::api::{ClientInput, ConfigChange, JiraCheck, JiraInput, JiraProject};
use cockpit_core::connections::AuthKind;
use cockpit_core::setup::{JIRA_TOKEN_PAGE, client_id_read, jira_site_read, projects_read};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::felder::{
    Abschnitt, Einleitung, Extern, Kontext, Kopierfeld, Meldung, Rueckmeldung, SchluesselFeld,
    konfig_speichern,
};
use crate::oauth_card::{OAuthCard, Service};
use crate::server::{self, CallError};

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
    // Das Ergebnis des Verbindungstests; erst danach erscheint die Wahl des Projekts.
    let ergebnis: RwSignal<Option<JiraCheck>> = RwSignal::new(None);
    let per_token = move || {
        ctx.store.health.with(|h| {
            h.as_ref().is_some_and(|h| {
                h.anmeldung.jira.connected && h.anmeldung.jira.kind == AuthKind::Token
            })
        })
    };
    let oauth_aktiv = move || {
        ctx.einr.with(|e| e.atlassian_client_id.is_some())
            || ctx.store.health.with(|h| {
                h.as_ref()
                    .is_some_and(|h| h.anmeldung.jira.kind == AuthKind::OAuth)
            })
    };
    view! {
        <VStack spacing="12px">
            <Einleitung title="Jira verbinden">
                <p>
                    "Damit zeigt dir Cockpit deine Tickets. Am einfachsten geht das mit einem „Token“: einem Passwort nur für Programme, das du in deinem Atlassian-Konto selbst erzeugst. Du brauchst drei Angaben und etwa zwei Minuten."
                </p>
                <p>
                    <strong>"Gut zu wissen:"</strong>
                    " Der Token hat die Rechte deines Kontos. Solange Cockpit nur lesen soll, nimm am besten ein Konto oder eine Rolle ohne Schreibrechte. Du kannst den Token jederzeit löschen."
                </p>
            </Einleitung>
            <JiraToken ctx=ctx ergebnis=ergebnis />
            {move || {
                (ergebnis.with(|e| e.as_ref().is_some_and(|e| e.ok)) || per_token())
                    .then(|| view! { <JiraProjektwahl ctx=ctx /> })
            }}
            <details class="ui-erweitert" open=oauth_aktiv>
                <summary>"Erweitert: Mit OAuth statt Token"</summary>
                <VStack spacing="12px">
                    <Einleitung title="Mit Atlassian-App (OAuth)">
                        <p>
                            "Du legst einmal eine eigene kleine „App“ in der Atlassian-Entwicklerkonsole an. Cockpit meldet dich damit an, ohne dass ein Passwort weitergegeben wird, und du kannst den Zugriff jederzeit entziehen. Mehr Klickarbeit, und deine Firma muss eigene Apps erlauben."
                        </p>
                    </Einleitung>
                    <JiraOauth ctx=ctx />
                </VStack>
            </details>
        </VStack>
    }
}

/// Ein Link im Look eines Knopfes, der in einem neuen Tab aufgeht.
#[component]
fn ExternKnopf(#[prop(into)] href: String, children: Children) -> impl IntoView {
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
fn JiraOauth(ctx: Kontext) -> impl IntoView {
    let client_id = RwSignal::new(String::new());
    vorbelegen(client_id, move || {
        ctx.einr
            .with(|e| e.atlassian_client_id.clone().unwrap_or_default())
    });
    let projekte = RwSignal::new(String::new());
    vorbelegen(projekte, move || {
        ctx.einr.with(|e| e.jira_projects.join(", "))
    });
    let rueck_id: Rueckmeldung = RwSignal::new(None);
    let rueck_projekte: Rueckmeldung = RwSignal::new(None);
    let callback = Signal::derive(move || ctx.einr.with(|e| e.callback_atlassian.clone()));

    let id_speichern = move |_| match client_id_read(&client_id.get_untracked()) {
        Ok(id) if id.is_empty() => rueck_id.set(Some((
            false,
            "Trage die Client-ID aus der Atlassian-Konsole ein.".into(),
        ))),
        Ok(id) => konfig_speichern(
            ctx,
            ConfigChange {
                atlassian: Some(ClientInput { client_id: id }),
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
            ConfigChange {
                jira: Some(JiraInput {
                    projects: Some(projects_read(&projekte.get_untracked())),
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
fn JiraToken(ctx: Kontext, ergebnis: RwSignal<Option<JiraCheck>>) -> impl IntoView {
    let site = RwSignal::new(String::new());
    vorbelegen(site, move || ctx.einr.with(|e| e.jira_site.clone()));
    let email = RwSignal::new(String::new());
    vorbelegen(email, move || ctx.einr.with(|e| e.jira_email.clone()));
    let rueck: Rueckmeldung = RwSignal::new(None);
    let laeuft = RwSignal::new(false);

    // Schlägt das Speichern fehl, läuft der Test nicht: den Knopf wieder freigeben.
    Effect::new(move |_| {
        if rueck.with(|r| r.as_ref().is_some_and(|(gut, _)| !gut)) {
            laeuft.set(false);
        }
    });

    // So versteht Cockpit die Eingabe: gleich unter dem Feld zu sehen.
    let site_gelesen = Signal::derive(move || jira_site_read(&site.get()));
    let verbunden = move || {
        ctx.store.health.with(|h| {
            h.as_ref().is_some_and(|h| {
                h.anmeldung.jira.connected && h.anmeldung.jira.kind == AuthKind::Token
            })
        })
    };

    let pruefen = move |_| {
        ergebnis.set(None);
        rueck.set(None);
        let site_neu = match site_gelesen.get_untracked() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                rueck.set(Some((
                    false,
                    "Trage zuerst die Adresse deiner Jira-Site ein.".into(),
                )));
                return;
            }
            Err(m) => {
                rueck.set(Some((false, m)));
                return;
            }
        };
        let email_neu = email.get_untracked().trim().to_string();
        if !email_neu.contains('@') || email_neu.contains(char::is_whitespace) {
            rueck.set(Some((
                false,
                "Trage die E-Mail-Adresse deines Atlassian-Kontos ein.".into(),
            )));
            return;
        }
        laeuft.set(true);
        // Erst Adresse und E-Mail sichern, dann der Test (nur Lesen).
        konfig_speichern(
            ctx,
            ConfigChange {
                jira: Some(JiraInput {
                    site: Some(site_neu),
                    email: Some(email_neu),
                    projects: None,
                }),
                ..Default::default()
            },
            rueck,
            "Adresse und E-Mail gespeichert.",
            move || {
                spawn_local(async move {
                    match server::post_jira_pruefen().await {
                        Ok(p) => ergebnis.set(Some(p)),
                        Err(e) => ergebnis.set(Some(JiraCheck {
                            message: e.message(),
                            ..Default::default()
                        })),
                    }
                    ctx.laden();
                    laeuft.set(false);
                });
            },
        );
    };

    view! {
        <Abschnitt title="1. Adresse deiner Jira-Site">
            <TextField
                label="Jira-Adresse"
                value=site
                placeholder="firma.atlassian.net"
                hint="So beginnt die Adresse, wenn du Jira im Browser öffnest. Du kannst sie auch einfach hineinkopieren – mit https:// und allem dahinter."
                autocomplete="off"
            />
            {move || match site_gelesen.get() {
                Ok(s) if !s.is_empty() => {
                    view! { <p class="ui-hinweis">"Cockpit verwendet: " <code>{s}</code></p> }.into_any()
                }
                Ok(_) => ().into_any(),
                Err(m) => {
                    view! { <p class="ui-hinweis" role="alert" style="color: var(--sys-color-red)">{m}</p> }
                        .into_any()
                }
            }}
        </Abschnitt>
        <Abschnitt title="2. E-Mail-Adresse deines Atlassian-Kontos">
            <TextField
                label="E-Mail-Adresse"
                value=email
                input_type="email"
                hint="Die Adresse, mit der du dich bei Atlassian anmeldest."
                autocomplete="off"
            />
        </Abschnitt>
        <Abschnitt title="3. Token erstellen">
            <ol class="ui-anleitung">
                <li>
                    "Klicke auf „Token-Seite öffnen“ und melde dich bei Atlassian an. Die Seite geht in einem neuen Tab auf."
                    <HStack spacing="12px" css="margin-top: 8px">
                        <ExternKnopf href=JIRA_TOKEN_PAGE>"Token-Seite öffnen"</ExternKnopf>
                    </HStack>
                </li>
                <li>"Klicke auf „Create API token“ (Token erstellen)."</li>
                <li>
                    "Gib einen Namen ein, zum Beispiel „Cockpit“, und wähle ein Ablaufdatum. Ein kürzeres ist sicherer; wenn es abläuft, erzeugst du einfach einen neuen."
                </li>
                <li>
                    "Klicke auf „Create“ und dann auf „Copy“ (Kopieren). Der Token wird nur jetzt angezeigt. Füge ihn gleich hier unten ein."
                </li>
            </ol>
            <SchluesselFeld
                ctx=ctx
                name="jira"
                label="Token"
                hinweis="Geheim wie ein Passwort. Er wird nur im Schlüsselbund dieses Rechners gespeichert und nie wieder angezeigt."
            />
        </Abschnitt>
        <Abschnitt title="4. Verbindung prüfen">
            <p class="t-subheadline t-secondary">
                "Cockpit speichert Adresse und E-Mail und fragt Jira, wer du bist. Dabei wird nur gelesen, nichts verändert."
            </p>
            <HStack spacing="12px">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=laeuft
                    on_click=pruefen
                >
                    "Verbindung prüfen"
                </Button>
                <Status tone=Signal::derive(move || {
                    if verbunden() { StatusTone::Green } else { StatusTone::Gray }
                })>{move || if verbunden() { "Verbunden – mit API-Token" } else { "Noch nicht geprüft" }}</Status>
            </HStack>
            <Meldung rueckmeldung=rueck />
            {move || {
                ergebnis
                    .get()
                    .map(|p| {
                        view! {
                            <p
                                class="ui-hinweis"
                                role=if p.ok { "status" } else { "alert" }
                                style=(!p.ok).then_some("color: var(--sys-color-red)")
                            >
                                {if p.ok { "✓ " } else { "" }}
                                {p.message}
                            </p>
                        }
                    })
            }}
        </Abschnitt>
    }
}

/// Nach dem Test: die Jira-Projekte zur Auswahl („Alle Projekte“ ist möglich). Gewählt wird wie in
/// der Inbox mit `Action::SelectJiraProject`; die Liste wird nur gelesen.
#[component]
fn JiraProjektwahl(ctx: Kontext) -> impl IntoView {
    let projekte = RwSignal::new(Vec::<JiraProject>::new());
    let rueck: Rueckmeldung = RwSignal::new(None);
    let geladen = RwSignal::new(false);
    spawn_local(async move {
        match server::fetch_jira_projects().await {
            Ok(r) => projekte.set(r.projects),
            Err(CallError::Unreachable) => {
                rueck.set(Some((false, CallError::Unreachable.message())))
            }
            Err(e) => rueck.set(Some((
                false,
                format!("Die Projekte konnten nicht geladen werden: {}", e.message()),
            ))),
        }
        geladen.set(true);
    });
    let optionen = Signal::derive(move || {
        let mut liste: Vec<(String, String)> = projekte.with(|p| {
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
        liste.sort_by_key(|a| a.1.to_lowercase());
        std::iter::once((String::new(), "Alle Projekte".to_string()))
            .chain(liste)
            .collect()
    });
    let gewaehlt = Signal::derive(move || {
        ctx.store
            .ws
            .with(|w| w.jira_project.clone().unwrap_or_default())
    });
    let wechseln = move |key: String| {
        let project = (!key.is_empty()).then_some(key);
        if let Err(e) = ctx.store.dispatch(Action::SelectJiraProject { project }) {
            rueck.set(Some((false, e.message())));
        } else {
            rueck.set(Some((true, "Auswahl gespeichert.".into())));
        }
    };
    view! {
        <Abschnitt title="Welche Projekte soll Cockpit zeigen?">
            <p class="t-subheadline t-secondary">
                "Wähle ein Projekt oder „Alle Projekte“. Du kannst das später in der Inbox ändern."
            </p>
            {move || {
                (!geladen.get())
                    .then(|| view! { <p class="t-footnote t-secondary">"Lade deine Projekte …"</p> })
            }}
            {move || {
                (geladen.get() && projekte.with(|p| p.is_empty()) && rueck.with(|r| r.is_none()))
                    .then(|| {
                        view! {
                            <p class="ui-hinweis">
                                "Jira zeigt für dieses Konto keine Projekte. Du kannst „Alle Projekte“ lassen."
                            </p>
                        }
                    })
            }}
            <Select
                options=optionen
                value=gewaehlt
                on_change=wechseln
                aria_label="Jira-Projekt"
            />
            <Meldung rueckmeldung=rueck />
        </Abschnitt>
    }
}
