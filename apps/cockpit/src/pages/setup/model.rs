//! Step 4: model – tool of the agent, endpoint (directly at Anthropic, own proxy such as
//! LiteLLM or another compatible endpoint), key, model name as a drop-down from the list of the
//! endpoint and the connection tests. The rules are in `cockpit_core::setup`.

use cockpit_core::api::{ConfigChange, EndpointInput, Provider};
use cockpit_core::setup::{
    EndpointKind, Tone, check_plain_text, model_hint, model_name_read, model_selection,
};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Intro, KeyField, Message, Section, config_save};
use crate::server::{self, CallError};

/// The text in the drop-down for a tool; those not implemented yet are shown as "bald".
fn provider_text(a: Provider) -> String {
    if a.implemented() {
        a.label().to_string()
    } else {
        format!("{} (bald)", a.label())
    }
}

#[component]
pub fn ModelStep(ctx: Context) -> impl IntoView {
    let provider = RwSignal::new(Provider::default());
    let art = RwSignal::new(EndpointKind::Direct);
    let url = RwSignal::new(String::new());
    let model = RwSignal::new(String::new());
    let from_hand = RwSignal::new(String::new());
    let list = RwSignal::new(Vec::<String>::new());
    let list_loads = RwSignal::new(false);
    let list_hint = RwSignal::new(None::<String>);
    // As soon as the person changes something themselves, the state from the server no longer overwrites their input.
    let touched = RwSignal::new(false);
    Effect::new(move |_| {
        let e = ctx.setup.get();
        if touched.get_untracked() {
            return;
        }
        let address = e.endpoint_url.unwrap_or_default();
        art.set(EndpointKind::from_url(&address));
        url.set(address);
        model.set(e.endpoint_model.unwrap_or_default());
        provider.set(e.provider);
    });
    let back: Feedback = RwSignal::new(None);
    let test_running = RwSignal::new(false);
    let test_error = RwSignal::new(None::<String>);

    // The model that is saved: typed by hand goes before the selection.
    let chosen_model = move || -> Result<String, String> {
        let hand = from_hand.get_untracked();
        if hand.trim().is_empty() {
            Ok(model.get_untracked().trim().to_string())
        } else {
            model_name_read(&hand)
        }
    };
    let input = move |model: Option<String>| EndpointInput {
        provider: Some(provider.get_untracked()),
        // Directly at Anthropic: no address.
        url: Some(if art.get_untracked().needs_address() {
            url.get_untracked()
        } else {
            String::new()
        }),
        model,
    };

    let save = move |_| match chosen_model() {
        Ok(m) => {
            let hand = !from_hand.get_untracked().trim().is_empty();
            config_save(
                ctx,
                ConfigChange {
                    endpoint: Some(input(Some(m.clone()))),
                    ..Default::default()
                },
                back,
                "Werkzeug, Endpunkt und Modell gespeichert.",
                move || {
                    if hand {
                        model.set(m.clone());
                        from_hand.set(String::new());
                    }
                },
            )
        }
        Err(m) => back.set(Some((false, m))),
    };
    // "Modelle laden": first save tool and address (the server asks the saved
    // endpoint with the key from the keychain), then fetch the list.
    let laden = move |_| {
        list_loads.set(true);
        list_hint.set(None);
        let change = ConfigChange {
            endpoint: Some(input(None)),
            ..Default::default()
        };
        spawn_local(async move {
            let error = |e: CallError| match e {
                CallError::Unreachable => {
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".to_string()
                }
                e => e.message(),
            };
            if let Err(e) = server::put_config(&change).await {
                list_hint.set(Some(error(e)));
                list_loads.set(false);
                return;
            }
            ctx.laden();
            match server::fetch_models().await {
                Ok(l) if l.loaded => {
                    list_hint.set(Some(format!("{} Modelle geladen.", l.models.len())));
                    list.set(l.models);
                }
                Ok(l) => list_hint.set(Some(l.hint.unwrap_or_else(|| {
                    "Der Endpunkt hat keine Modelle geliefert. Trage den Namen von Hand ein.".into()
                }))),
                Err(e) => list_hint.set(Some(error(e))),
            }
            list_loads.set(false);
        });
    };
    let test = move |_| {
        test_running.set(true);
        test_error.set(None);
        spawn_local(async move {
            match server::fetch_endpoint_check().await {
                Ok(p) => ctx.check.set(Some(p)),
                Err(CallError::Unreachable) => test_error.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => test_error.set(Some(e.message())),
            }
            test_running.set(false);
        });
    };
    let c_running = RwSignal::new(false);
    let c_error = RwSignal::new(None::<String>);
    let from_container = move |_| {
        c_running.set(true);
        c_error.set(None);
        spawn_local(async move {
            match server::fetch_container_check().await {
                Ok(p) => ctx.container_check.set(Some(p)),
                Err(CallError::Unreachable) => c_error.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => c_error.set(Some(e.message())),
            }
            c_running.set(false);
        });
    };
    let ready = move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.endpoint.configured && h.endpoint.key)
        })
    };

    let key_set = move || ctx.setup.with(|e| e.has_key("agent-key"));
    let provider_options = Signal::derive(move || {
        Provider::ALL
            .into_iter()
            .map(|a| (a.id().to_string(), provider_text(a)))
            .collect::<Vec<_>>()
    });
    let kind_options = Signal::derive(move || {
        EndpointKind::ALL
            .into_iter()
            .map(|a| (a.id().to_string(), a.label().to_string()))
            .collect::<Vec<_>>()
    });
    let model_options = Signal::derive(move || {
        let l = list.get();
        let mut o: Vec<(String, String)> = Vec::new();
        let chosen = model.get();
        if chosen.trim().is_empty() {
            o.push((String::new(), "– Modell wählen –".into()));
        }
        o.extend(model_selection(&l, &chosen).into_iter().map(|m| {
            // "Nicht in der Liste" only applies if there is a list at all.
            let text = if m.unknown && !l.is_empty() {
                format!("{} (nicht in der Liste)", m.id)
            } else {
                m.id.clone()
            };
            (m.id, text)
        }));
        o
    });

    view! {
        <VStack spacing="12px">
            <Intro title="Modell für den Agenten">
                <p>
                    "Der Agent denkt mit einem KI-Modell. Hier legst du fest, mit welchem Werkzeug er arbeitet und wohin er seine Modellanfragen schickt: direkt zu Anthropic, über einen Proxy deines Teams (zum Beispiel LiteLLM) oder zu einem anderen kompatiblen Endpunkt."
                </p>
                <p>
                    <strong>"Das kannst du nicht selbst erzeugen:"</strong>
                    " Den Key bekommst du von Anthropic (Konsole) oder vom Betreiber deines Proxys, dazu bei einem Proxy die Adresse. Hast du sie noch nicht, überspringe diesen Schritt und hole ihn später nach."
                </p>
            </Intro>
            <Section title="1. Werkzeug">
                <Select
                    label="Werkzeug des Agenten"
                    options=provider_options
                    value=Signal::derive(move || provider.get().id().to_string())
                    on_change=Callback::new(move |v: String| {
                        if let Some(a) = Provider::from_id(&v) {
                            touched.set(true);
                            provider.set(a);
                        }
                    })
                />
                {move || {
                    provider
                        .get()
                        .not_implemented()
                        .map(|m| {
                            view! {
                                <p class="ui-hinweis" role="status">
                                    {m}
                                    " Du kannst es schon eintragen; Läufe starten erst, wenn es umgesetzt ist."
                                </p>
                            }
                        })
                }}
            </Section>
            <Section title="2. Endpunkt">
                <Select
                    label="Wohin gehen die Modellanfragen?"
                    options=kind_options
                    value=Signal::derive(move || art.get().id().to_string())
                    on_change=Callback::new(move |v: String| {
                        if let Some(a) = EndpointKind::from_id(&v) {
                            touched.set(true);
                            art.set(a);
                        }
                    })
                />
                <p class="t-footnote t-secondary">{move || art.get().explanation()}</p>
                {move || {
                    art.get()
                        .needs_address()
                        .then(|| {
                            view! {
                                <TextField
                                    label="Adresse des Endpunkts"
                                    value=url
                                    placeholder=art.get().url_placeholder()
                                    hint="Beginnt meist mit https://. Ohne Angabe ergänzt Cockpit http://."
                                    autocomplete="off"
                                />
                            }
                        })
                }}
            </Section>
            <Section title="3. Key">
                {move || {
                    view! {
                        <KeyField
                            ctx=ctx
                            name="agent-key"
                            label=art.get().key_label()
                            hint="Geheim wie ein Passwort. Er wird nur im Schlüsselbund dieses Rechners gespeichert."
                        />
                    }
                }}
            </Section>
            <Section title="4. Modell">
                <p class="t-subheadline t-secondary">
                    "Cockpit fragt den Endpunkt nach seinen Modellen und zeigt sie als Liste. Dafür muss der Key schon abgelegt sein. Beim Laden speichert Cockpit Werkzeug und Adresse."
                </p>
                <HStack spacing="12px">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || {
                            list_loads.get() || !key_set()
                                || (art.get().needs_address() && url.with(|u| u.trim().is_empty()))
                        })
                        on_click=laden
                    >
                        {move || if list_loads.get() { "Lade …" } else { "Modelle laden" }}
                    </Button>
                </HStack>
                {move || {
                    (!key_set())
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    "Lege zuerst den Key ab (Abschnitt 3)."
                                </p>
                            }
                        })
                }}
                {move || {
                    list_hint
                        .get()
                        .map(|m| view! { <p class="ui-hinweis" role="status">{m}</p> })
                }}
                <Select
                    label="Modell"
                    options=model_options
                    value=Signal::derive(move || model.get())
                    on_change=Callback::new(move |v: String| {
                        touched.set(true);
                        model.set(v);
                    })
                    disabled=Signal::derive(move || {
                        list.with(Vec::is_empty) && model.with(|m| m.trim().is_empty())
                    })
                />
                {move || {
                    model_hint(&list.get(), &model.get())
                        .map(|m| view! { <p class="ui-hinweis" role="status">{m}</p> })
                }}
                <TextField
                    label="Modellname von Hand"
                    value=from_hand
                    placeholder="claude-sonnet-5-5"
                    hint="Nur nötig, wenn die Liste fehlt oder das Modell nicht darin steht. Steht hier etwas, gilt es statt der Auswahl. Genau abtippen, auf Groß- und Kleinschreibung achten."
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=save>
                        "Werkzeug, Endpunkt und Modell speichern"
                    </Button>
                </HStack>
                <Message feedback=back />
            </Section>
            <Section title="5. Verbindung testen">
                <p class="t-subheadline t-secondary">
                    "Cockpit fragt den Endpunkt nach seiner Modellliste und meldet, ob er antwortet, den Key annimmt und das Modell kennt. Dabei wird nichts berechnet."
                </p>
                <HStack spacing="12px">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || test_running.get() || !ready())
                        on_click=test
                    >
                        {move || if test_running.get() { "Teste …" } else { "Verbindung testen" }}
                    </Button>
                </HStack>
                {move || {
                    (!ready())
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    "Der Test geht, sobald Endpunkt, Modell und Key gespeichert sind."
                                </p>
                            }
                        })
                }}
                {move || {
                    test_error
                        .get()
                        .map(|m| view! { <p class="ui-hinweis" role="alert">{m}</p> })
                }}
                {move || {
                    ctx.check
                        .get()
                        .map(|p| {
                            let (ton, text) = check_plain_text(&p);
                            let tone = match ton {
                                Tone::Good => StatusTone::Green,
                                Tone::Open => StatusTone::Gray,
                                Tone::Problem => StatusTone::Red,
                            };
                            view! {
                                <HStack spacing="8px">
                                    <Status tone=tone>{if ton == Tone::Good { "Gelungen" } else { "Nicht gelungen" }}</Status>
                                    <span class="t-subheadline">{text}</span>
                                </HStack>
                            }
                        })
                }}
            </Section>
            <Section title="6. Aus dem Container erreichbar?">
                <p class="t-subheadline t-secondary">
                    "Der Agent läuft in einem Container, der den Endpunkt womöglich anders sieht als dein Mac – etwa, wenn ein Proxy nur im VPN liegt. Dieser Test startet kurz einen Container und fragt den Endpunkt von dort aus. Er braucht das gebaute Agent-Image (Schritt „Container“) und kostet nichts."
                </p>
                <HStack spacing="12px">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || c_running.get() || !ready())
                        on_click=from_container
                    >
                        {move || if c_running.get() { "Teste im Container …" } else { "Aus dem Container testen" }}
                    </Button>
                </HStack>
                {move || {
                    c_error
                        .get()
                        .map(|m| view! { <p class="ui-hinweis" role="alert">{m}</p> })
                }}
                {move || {
                    ctx.container_check
                        .get()
                        .map(|p| {
                            let gut = p.checked && p.container_reached && p.key_ok;
                            let tone = if gut {
                                StatusTone::Green
                            } else if p.checked {
                                StatusTone::Red
                            } else {
                                StatusTone::Gray
                            };
                            view! {
                                <HStack spacing="8px">
                                    <Status tone=tone>
                                        {if gut { "Gelungen" } else if p.checked { "Nicht gelungen" } else { "Nicht geprüft" }}
                                    </Status>
                                    <span class="t-subheadline">{p.hint.unwrap_or_default()}</span>
                                </HStack>
                            }
                        })
                }}
            </Section>
        </VStack>
    }
}
