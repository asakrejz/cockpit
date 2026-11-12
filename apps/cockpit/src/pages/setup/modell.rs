//! Schritt 4: Modell – Werkzeug des Agenten, Endpunkt (direkt bei Anthropic, eigener Proxy wie
//! LiteLLM oder ein anderer kompatibler Endpunkt), Key, Modellname als Drop-down aus der Liste des
//! Endpunkts und die Verbindungstests. Die Regeln stehen in `cockpit_core::einrichten`.

use cockpit_core::api::{Provider, EndpointInput, ConfigChange};
use cockpit_core::setup::{
    EndpointKind, Tone, model_selection, model_hint, model_name_read, check_plain_text,
};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::felder::{
    Abschnitt, Einleitung, Kontext, Meldung, Rueckmeldung, SchluesselFeld, konfig_speichern,
};
use crate::server::{self, CallError};

/// Der Text im Drop-down für ein Werkzeug; die noch nicht umgesetzten stehen als „bald“ da.
fn anbieter_text(a: Provider) -> String {
    if a.implemented() {
        a.label().to_string()
    } else {
        format!("{} (bald)", a.label())
    }
}

#[component]
pub fn ModellSchritt(ctx: Kontext) -> impl IntoView {
    let anbieter = RwSignal::new(Provider::default());
    let art = RwSignal::new(EndpointKind::Direct);
    let url = RwSignal::new(String::new());
    let modell = RwSignal::new(String::new());
    let von_hand = RwSignal::new(String::new());
    let liste = RwSignal::new(Vec::<String>::new());
    let liste_laedt = RwSignal::new(false);
    let liste_hinweis = RwSignal::new(None::<String>);
    // Sobald die Person selbst etwas ändert, überschreibt der Stand vom Server ihre Eingabe nicht mehr.
    let angefasst = RwSignal::new(false);
    Effect::new(move |_| {
        let e = ctx.einr.get();
        if angefasst.get_untracked() {
            return;
        }
        let adresse = e.endpoint_url.unwrap_or_default();
        art.set(EndpointKind::from_url(&adresse));
        url.set(adresse);
        modell.set(e.endpoint_model.unwrap_or_default());
        anbieter.set(e.provider);
    });
    let rueck: Rueckmeldung = RwSignal::new(None);
    let test_laeuft = RwSignal::new(false);
    let test_fehler = RwSignal::new(None::<String>);

    // Das Modell, das gespeichert wird: von Hand getippt geht der Auswahl vor.
    let gewaehltes_modell = move || -> Result<String, String> {
        let hand = von_hand.get_untracked();
        if hand.trim().is_empty() {
            Ok(modell.get_untracked().trim().to_string())
        } else {
            model_name_read(&hand)
        }
    };
    let eingabe = move |modell: Option<String>| EndpointInput {
        provider: Some(anbieter.get_untracked()),
        // Direkt bei Anthropic: keine Adresse.
        url: Some(if art.get_untracked().needs_address() {
            url.get_untracked()
        } else {
            String::new()
        }),
        model: modell,
    };

    let speichern = move |_| match gewaehltes_modell() {
        Ok(m) => {
            let hand = !von_hand.get_untracked().trim().is_empty();
            konfig_speichern(
                ctx,
                ConfigChange {
                    endpoint: Some(eingabe(Some(m.clone()))),
                    ..Default::default()
                },
                rueck,
                "Werkzeug, Endpunkt und Modell gespeichert.",
                move || {
                    if hand {
                        modell.set(m.clone());
                        von_hand.set(String::new());
                    }
                },
            )
        }
        Err(m) => rueck.set(Some((false, m))),
    };
    // „Modelle laden“: erst Werkzeug und Adresse speichern (der Server fragt den gespeicherten
    // Endpunkt mit dem Key aus dem Schlüsselbund), dann die Liste holen.
    let laden = move |_| {
        liste_laedt.set(true);
        liste_hinweis.set(None);
        let aenderung = ConfigChange {
            endpoint: Some(eingabe(None)),
            ..Default::default()
        };
        spawn_local(async move {
            let fehler = |e: CallError| match e {
                CallError::Unreachable => {
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".to_string()
                }
                e => e.message(),
            };
            if let Err(e) = server::put_konfiguration(&aenderung).await {
                liste_hinweis.set(Some(fehler(e)));
                liste_laedt.set(false);
                return;
            }
            ctx.laden();
            match server::fetch_modelle().await {
                Ok(l) if l.loaded => {
                    liste_hinweis.set(Some(format!("{} Modelle geladen.", l.models.len())));
                    liste.set(l.models);
                }
                Ok(l) => liste_hinweis.set(Some(l.hint.unwrap_or_else(|| {
                    "Der Endpunkt hat keine Modelle geliefert. Trage den Namen von Hand ein.".into()
                }))),
                Err(e) => liste_hinweis.set(Some(fehler(e))),
            }
            liste_laedt.set(false);
        });
    };
    let testen = move |_| {
        test_laeuft.set(true);
        test_fehler.set(None);
        spawn_local(async move {
            match server::fetch_endpunkt_check().await {
                Ok(p) => ctx.pruefung.set(Some(p)),
                Err(CallError::Unreachable) => test_fehler.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => test_fehler.set(Some(e.message())),
            }
            test_laeuft.set(false);
        });
    };
    let c_laeuft = RwSignal::new(false);
    let c_fehler = RwSignal::new(None::<String>);
    let aus_container = move |_| {
        c_laeuft.set(true);
        c_fehler.set(None);
        spawn_local(async move {
            match server::fetch_container_check().await {
                Ok(p) => ctx.container_pruefung.set(Some(p)),
                Err(CallError::Unreachable) => c_fehler.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => c_fehler.set(Some(e.message())),
            }
            c_laeuft.set(false);
        });
    };
    let bereit = move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.endpunkt.configured && h.endpunkt.key)
        })
    };

    let key_gesetzt = move || ctx.einr.with(|e| e.has_key("agent-key"));
    let anbieter_optionen = Signal::derive(move || {
        Provider::ALL
            .into_iter()
            .map(|a| (a.id().to_string(), anbieter_text(a)))
            .collect::<Vec<_>>()
    });
    let art_optionen = Signal::derive(move || {
        EndpointKind::ALL
            .into_iter()
            .map(|a| (a.id().to_string(), a.label().to_string()))
            .collect::<Vec<_>>()
    });
    let modell_optionen = Signal::derive(move || {
        let l = liste.get();
        let mut o: Vec<(String, String)> = Vec::new();
        let gewaehlt = modell.get();
        if gewaehlt.trim().is_empty() {
            o.push((String::new(), "– Modell wählen –".into()));
        }
        o.extend(model_selection(&l, &gewaehlt).into_iter().map(|m| {
            // „Nicht in der Liste“ gilt nur, wenn es überhaupt eine Liste gibt.
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
            <Einleitung title="Modell für den Agenten">
                <p>
                    "Der Agent denkt mit einem KI-Modell. Hier legst du fest, mit welchem Werkzeug er arbeitet und wohin er seine Modellanfragen schickt: direkt zu Anthropic, über einen Proxy deines Teams (zum Beispiel LiteLLM) oder zu einem anderen kompatiblen Endpunkt."
                </p>
                <p>
                    <strong>"Das kannst du nicht selbst erzeugen:"</strong>
                    " Den Key bekommst du von Anthropic (Konsole) oder vom Betreiber deines Proxys, dazu bei einem Proxy die Adresse. Hast du sie noch nicht, überspringe diesen Schritt und hole ihn später nach."
                </p>
            </Einleitung>
            <Abschnitt title="1. Werkzeug">
                <Select
                    label="Werkzeug des Agenten"
                    options=anbieter_optionen
                    value=Signal::derive(move || anbieter.get().id().to_string())
                    on_change=Callback::new(move |v: String| {
                        if let Some(a) = Provider::from_id(&v) {
                            angefasst.set(true);
                            anbieter.set(a);
                        }
                    })
                />
                {move || {
                    anbieter
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
            </Abschnitt>
            <Abschnitt title="2. Endpunkt">
                <Select
                    label="Wohin gehen die Modellanfragen?"
                    options=art_optionen
                    value=Signal::derive(move || art.get().id().to_string())
                    on_change=Callback::new(move |v: String| {
                        if let Some(a) = EndpointKind::from_id(&v) {
                            angefasst.set(true);
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
            </Abschnitt>
            <Abschnitt title="3. Key">
                {move || {
                    view! {
                        <SchluesselFeld
                            ctx=ctx
                            name="agent-key"
                            label=art.get().key_label()
                            hinweis="Geheim wie ein Passwort. Er wird nur im Schlüsselbund dieses Rechners gespeichert."
                        />
                    }
                }}
            </Abschnitt>
            <Abschnitt title="4. Modell">
                <p class="t-subheadline t-secondary">
                    "Cockpit fragt den Endpunkt nach seinen Modellen und zeigt sie als Liste. Dafür muss der Key schon abgelegt sein. Beim Laden speichert Cockpit Werkzeug und Adresse."
                </p>
                <HStack spacing="12px">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || {
                            liste_laedt.get() || !key_gesetzt()
                                || (art.get().needs_address() && url.with(|u| u.trim().is_empty()))
                        })
                        on_click=laden
                    >
                        {move || if liste_laedt.get() { "Lade …" } else { "Modelle laden" }}
                    </Button>
                </HStack>
                {move || {
                    (!key_gesetzt())
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    "Lege zuerst den Key ab (Abschnitt 3)."
                                </p>
                            }
                        })
                }}
                {move || {
                    liste_hinweis
                        .get()
                        .map(|m| view! { <p class="ui-hinweis" role="status">{m}</p> })
                }}
                <Select
                    label="Modell"
                    options=modell_optionen
                    value=Signal::derive(move || modell.get())
                    on_change=Callback::new(move |v: String| {
                        angefasst.set(true);
                        modell.set(v);
                    })
                    disabled=Signal::derive(move || {
                        liste.with(Vec::is_empty) && modell.with(|m| m.trim().is_empty())
                    })
                />
                {move || {
                    model_hint(&liste.get(), &modell.get())
                        .map(|m| view! { <p class="ui-hinweis" role="status">{m}</p> })
                }}
                <TextField
                    label="Modellname von Hand"
                    value=von_hand
                    placeholder="claude-sonnet-5-5"
                    hint="Nur nötig, wenn die Liste fehlt oder das Modell nicht darin steht. Steht hier etwas, gilt es statt der Auswahl. Genau abtippen, auf Groß- und Kleinschreibung achten."
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=speichern>
                        "Werkzeug, Endpunkt und Modell speichern"
                    </Button>
                </HStack>
                <Meldung rueckmeldung=rueck />
            </Abschnitt>
            <Abschnitt title="5. Verbindung testen">
                <p class="t-subheadline t-secondary">
                    "Cockpit fragt den Endpunkt nach seiner Modellliste und meldet, ob er antwortet, den Key annimmt und das Modell kennt. Dabei wird nichts berechnet."
                </p>
                <HStack spacing="12px">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || test_laeuft.get() || !bereit())
                        on_click=testen
                    >
                        {move || if test_laeuft.get() { "Teste …" } else { "Verbindung testen" }}
                    </Button>
                </HStack>
                {move || {
                    (!bereit())
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    "Der Test geht, sobald Endpunkt, Modell und Key gespeichert sind."
                                </p>
                            }
                        })
                }}
                {move || {
                    test_fehler
                        .get()
                        .map(|m| view! { <p class="ui-hinweis" role="alert">{m}</p> })
                }}
                {move || {
                    ctx.pruefung
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
            </Abschnitt>
            <Abschnitt title="6. Aus dem Container erreichbar?">
                <p class="t-subheadline t-secondary">
                    "Der Agent läuft in einem Container, der den Endpunkt womöglich anders sieht als dein Mac – etwa, wenn ein Proxy nur im VPN liegt. Dieser Test startet kurz einen Container und fragt den Endpunkt von dort aus. Er braucht das gebaute Agent-Image (Schritt „Container“) und kostet nichts."
                </p>
                <HStack spacing="12px">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || c_laeuft.get() || !bereit())
                        on_click=aus_container
                    >
                        {move || if c_laeuft.get() { "Teste im Container …" } else { "Aus dem Container testen" }}
                    </Button>
                </HStack>
                {move || {
                    c_fehler
                        .get()
                        .map(|m| view! { <p class="ui-hinweis" role="alert">{m}</p> })
                }}
                {move || {
                    ctx.container_pruefung
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
            </Abschnitt>
        </VStack>
    }
}
