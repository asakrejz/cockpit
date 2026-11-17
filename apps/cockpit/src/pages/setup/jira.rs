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

use super::auth_method::{AuthMethodPicker, current};
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
        jira_default_method(&status, ctx.setup.with(|e| e.atlassian_client_id.is_some()))
    });
    let method = current(chosen, default);
    // Both ways stay in the page (only hidden), so what was typed in one is still there after
    // switching back and forth.
    view! {
        <VStack spacing="12px">
            <Intro title="Jira verbinden">
                <p>
                    "Damit zeigt dir Cockpit deine Tickets. Du kannst dich auf zwei Wegen anmelden – wähle oben, welchen du nutzen willst. Der API-Token ist der einfachere: ein Passwort nur für Programme, das du in deinem Atlassian-Konto selbst erzeugst."
                </p>
                <p>
                    <strong>"Gut zu wissen:"</strong>
                    " Der Zugang hat die Rechte deines Kontos. Solange Cockpit nur lesen soll, nimm am besten ein Konto oder eine Rolle ohne Schreibrechte. Du kannst den Zugang jederzeit entziehen."
                </p>
            </Intro>
            <AuthMethodPicker chosen=chosen default=default aria_label="Anmeldeweg für Jira" />
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
                    <Intro title="Mit Atlassian-App (OAuth)">
                        <p>
                            "Du legst einmal eine eigene kleine „App“ in der Atlassian-Entwicklerkonsole an. Cockpit meldet dich damit an, ohne dass ein Passwort weitergegeben wird, und du kannst den Zugriff jederzeit entziehen. Mehr Klickarbeit, und deine Firma muss eigene Apps erlauben."
                        </p>
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
        Ok(id) if id.is_empty() => back_id.set(Some((
            false,
            "Trage die Client-ID aus der Atlassian-Konsole ein.".into(),
        ))),
        Ok(id) => config_save(
            ctx,
            ConfigChange {
                atlassian: Some(ClientInput { client_id: id }),
                ..Default::default()
            },
            back_id,
            "Client-ID gespeichert.",
            || {},
        ),
        Err(m) => back_id.set(Some((false, m))),
    };
    let site_save = move |_| {
        config_save(
            ctx,
            ConfigChange {
                atlassian_site: Some(site.get_untracked()),
                ..Default::default()
            },
            back_site,
            "Site gespeichert.",
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
                    back_id.set(Some((
                        true,
                        "Client-ID und Client-Secret sind entfernt.".into(),
                    )));
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
            "Projekte gespeichert.",
            || {},
        )
    };

    view! {
        <Section title="1. App in der Atlassian-Entwicklerkonsole anlegen">
            <ol class="ui-instructions">
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
                    <ul class="ui-checklist">
                        <li><code>"read:jira-work"</code><span class="ui-checklist__permission">"Tickets lesen"</span></li>
                        <li><code>"write:jira-work"</code><span class="ui-checklist__permission">"Tickets kommentieren und Status ändern"</span></li>
                        <li><code>"read:jira-user"</code><span class="ui-checklist__permission">"Namen von Personen lesen"</span></li>
                    </ul>
                </li>
                <li>
                    "Wähle links „Authorization“, bei „OAuth 2.0 (3LO)“ „Add“ (oder „Configure“) und trage als „Callback URL“ diese Adresse ein. Dann „Save changes“:"
                    <CopyField store=ctx.store label="Callback-URL" text=callback />
                </li>
                <li>
                    "Wähle links „Settings“. Dort stehen die „Client ID“ und das „Secret“. Beide trägst du gleich unten ein."
                </li>
            </ol>
        </Section>
        <Section title="2. Client-ID und Client-Secret eintragen">
            <TextField
                label="Client ID"
                value=client_id
                hint="Nicht geheim. Kopiere sie genau so, wie sie in der Konsole steht."
                autocomplete="off"
            />
            <HStack spacing="12px" css="flex-wrap: wrap">
                <Button variant=ButtonStyle::GlassProminent on_click=id_save>
                    "Client-ID speichern"
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
                                    "Client-ID entfernen"
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
                                "Zum Entfernen trenne zuerst unten bei „4. Anmelden“ die Verbindung (dort kannst du auch gleich Client-ID und Secret vergessen lassen)."
                            </p>
                        }
                    })
            }}
            {move || {
                confirm_remove
                    .get()
                    .then(|| {
                        view! {
                            <div class="ui-hint" role="alertdialog" aria-label="Client-ID entfernen">
                                <p>
                                    "Wirklich entfernen? Die Client-ID wird aus der Konfiguration gelöscht und das Client-Secret aus der .env-Datei. Danach musst du beides neu eintragen, um dich per OAuth anzumelden."
                                </p>
                                <HStack spacing="12px" css="margin-top: 8px">
                                    <Button destructive=true disabled=removing on_click=remove>
                                        "Ja, entfernen"
                                    </Button>
                                    <Button on_click=move |_| confirm_remove.set(false)>"Abbrechen"</Button>
                                </HStack>
                            </div>
                        }
                    })
            }}
            <Message feedback=back_id />
            <KeyField
                ctx=ctx
                name="atlassian-oauth"
                label="Client-Secret (Secret)"
                hint="Geheim wie ein Passwort. Es wird nur in der .env-Datei im Projektordner dieses Rechners gespeichert und nie wieder angezeigt."
            />
        </Section>
        <Section title="Site wählen (nur bei mehreren)">
            <TextField
                label="Site"
                value=site
                hint="Nur nötig, wenn dein Atlassian-Konto mehrere Sites freigibt. Name, Adresse oder ID der Site. Leer lassen: Cockpit nimmt die einzige Site. Gilt nach dem nächsten Anmelden."
                autocomplete="off"
            />
            <HStack spacing="12px">
                <Button
                    on_click=site_save
                    disabled=Signal::derive(move || !id_stored.get())
                >
                    "Site speichern"
                </Button>
            </HStack>
            <Message feedback=back_site />
        </Section>
        <Section title="3. Welche Projekte? (optional)">
            <TextField
                label="Jira-Projekte"
                value=projects
                hint="Kürzel mit Komma getrennt, z. B. BAU, WEB. Empty lassen: Cockpit zeigt dann deine Tickets und die, die du beobachtest. In der Inbox kannst du später ein Projekt wählen."
                autocomplete="off"
            />
            <HStack spacing="12px">
                <Button on_click=projects_save>"Projekte speichern"</Button>
            </HStack>
            <Message feedback=back_projects />
        </Section>
        <Section title="4. Anmelden">
            <p class="t-subheadline t-secondary">
                "Es öffnet sich eine Seite von Atlassian. Prüfe dort den Namen deiner Site und bestätige mit „Accept“."
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
                back.set(Some((
                    false,
                    "Trage zuerst die Adresse deiner Jira-Site ein.".into(),
                )));
                return;
            }
            Err(m) => {
                back.set(Some((false, m)));
                return;
            }
        };
        let email_new = email.get_untracked().trim().to_string();
        if !email_new.contains('@') || email_new.contains(char::is_whitespace) {
            back.set(Some((
                false,
                "Trage die E-Mail-Adresse deines Atlassian-Kontos ein.".into(),
            )));
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
            "Adresse und E-Mail gespeichert.",
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
        <Section title="1. Adresse deiner Jira-Site">
            <TextField
                label="Jira-Adresse"
                value=site
                placeholder="firma.atlassian.net"
                hint="So beginnt die Adresse, wenn du Jira im Browser öffnest. Du kannst sie auch einfach hineinkopieren – mit https:// und allem dahinter."
                autocomplete="off"
            />
            {move || match site_read.get() {
                Ok(s) if !s.is_empty() => {
                    view! { <p class="ui-hint">"Cockpit verwendet: " <code>{s}</code></p> }.into_any()
                }
                Ok(_) => ().into_any(),
                Err(m) => {
                    view! { <p class="ui-hint" role="alert" style="color: var(--sys-color-red)">{m}</p> }
                        .into_any()
                }
            }}
        </Section>
        <Section title="2. E-Mail-Adresse deines Atlassian-Kontos">
            <TextField
                label="E-Mail-Adresse"
                value=email
                input_type="email"
                hint="Die Adresse, mit der du dich bei Atlassian anmeldest."
                autocomplete="off"
            />
        </Section>
        <Section title="3. Token erstellen">
            <ol class="ui-instructions">
                <li>
                    "Klicke auf „Token-Seite öffnen“ und melde dich bei Atlassian an. Die Seite geht in einem neuen Tab auf."
                    <HStack spacing="12px" css="margin-top: 8px">
                        <ExternButton href=JIRA_TOKEN_PAGE>"Token-Seite öffnen"</ExternButton>
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
            <KeyField
                ctx=ctx
                name="jira"
                label="Token"
                hint="Geheim wie ein Passwort. Er wird nur in der .env-Datei im Projektordner dieses Rechners gespeichert und nie wieder angezeigt."
            />
        </Section>
        <Section title="4. Verbindung prüfen">
            <p class="t-subheadline t-secondary">
                "Cockpit speichert Adresse und E-Mail und fragt Jira, wer du bist. Dabei wird nur gelesen, nichts verändert."
            </p>
            <HStack spacing="12px">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=running
                    on_click=check
                >
                    "Verbindung prüfen"
                </Button>
                <Status tone=Signal::derive(move || {
                    if connected() { StatusTone::Green } else { StatusTone::Gray }
                })>{move || if connected() { "Verbunden – mit API-Token" } else { "Noch nicht geprüft" }}</Status>
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
                                {p.message}
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
                format!("Die Projekte konnten nicht geladen werden: {}", e.message()),
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
        std::iter::once((String::new(), "Alle Projekte".to_string()))
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
            back.set(Some((false, e.message())));
        } else {
            back.set(Some((true, "Auswahl gespeichert.".into())));
        }
    };
    view! {
        <Section title="Welche Projekte soll Cockpit zeigen?">
            <p class="t-subheadline t-secondary">
                "Wähle ein Projekt oder „Alle Projekte“. Du kannst das später in der Inbox ändern."
            </p>
            {move || {
                (!loaded.get())
                    .then(|| view! { <p class="t-footnote t-secondary">"Lade deine Projekte …"</p> })
            }}
            {move || {
                (loaded.get() && projects.with(|p| p.is_empty()) && back.with(|r| r.is_none()))
                    .then(|| {
                        view! {
                            <p class="ui-hint">
                                "Jira zeigt für dieses Konto keine Projekte. Du kannst „Alle Projekte“ lassen."
                            </p>
                        }
                    })
            }}
            <Select
                options=options
                value=chosen
                on_change=switch
                aria_label="Jira-Projekt"
            />
            <Message feedback=back />
        </Section>
    }
}
