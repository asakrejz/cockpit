//! Schritt 4: Modellzugang (LiteLLM) – Adresse, Modell, Key und der Verbindungstest.

use cockpit_core::api::{KonfigAenderung, LitellmEingabe};
use cockpit_core::einrichten::{Ton, pruefung_klartext};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::felder::{
    Abschnitt, Einleitung, Kontext, Meldung, Rueckmeldung, SchluesselFeld, konfig_speichern,
};
use crate::server::{self, CallError};

#[component]
pub fn ModellSchritt(ctx: Kontext) -> impl IntoView {
    let url = RwSignal::new(String::new());
    let modell = RwSignal::new(String::new());
    Effect::new(move |_| {
        let e = ctx.einr.get();
        if url.get_untracked().is_empty() {
            url.set(e.litellm_url.unwrap_or_default());
        }
        if modell.get_untracked().is_empty() {
            modell.set(e.litellm_modell.unwrap_or_default());
        }
    });
    let rueck: Rueckmeldung = RwSignal::new(None);
    let test_laeuft = RwSignal::new(false);
    let test_fehler = RwSignal::new(None::<String>);

    let speichern = move |_| {
        konfig_speichern(
            ctx,
            KonfigAenderung {
                litellm: Some(LitellmEingabe {
                    url: url.get_untracked(),
                    modell: Some(modell.get_untracked()),
                }),
                ..Default::default()
            },
            rueck,
            "Adresse und Modell gespeichert.",
            || {},
        )
    };
    let testen = move |_| {
        test_laeuft.set(true);
        test_fehler.set(None);
        spawn_local(async move {
            match server::fetch_litellm_check().await {
                Ok(p) => ctx.pruefung.set(Some(p)),
                Err(CallError::Unreachable) => test_fehler.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => test_fehler.set(Some(e.message())),
            }
            test_laeuft.set(false);
        });
    };
    let bereit = move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.litellm.konfiguriert && h.litellm.key)
        })
    };

    view! {
        <VStack spacing="12px">
            <Einleitung title="Modellzugang (LiteLLM)">
                <p>
                    "Der Agent denkt mit einem KI-Modell. Cockpit spricht es über einen „LiteLLM-Proxy“ an – eine Zwischenstation, die dein Team oder deine IT betreibt."
                </p>
                <p>
                    <strong>"Das kannst du nicht selbst erzeugen:"</strong>
                    " Frag den Betreiber nach drei Dingen: der Adresse des Proxys, einem persönlichen Key und dem genauen Namen des Modells. Hast du sie noch nicht, überspringe diesen Schritt und hole ihn später nach."
                </p>
            </Einleitung>
            <Abschnitt title="1. Adresse und Modell">
                <TextField
                    label="Adresse des Proxys"
                    value=url
                    placeholder="http://localhost:4000"
                    hint="Beginnt meist mit https://. Ohne Angabe ergänzt Cockpit http://."
                    autocomplete="off"
                />
                <TextField
                    label="Modellname"
                    value=modell
                    placeholder="claude-sonnet-5-5"
                    hint="So heißt das Modell im Proxy – genau abtippen, auf Groß- und Kleinschreibung achten."
                    autocomplete="off"
                />
                <HStack spacing="12px">
                    <Button variant=ButtonStyle::GlassProminent on_click=speichern>
                        "Adresse und Modell speichern"
                    </Button>
                </HStack>
                <Meldung rueckmeldung=rueck />
            </Abschnitt>
            <Abschnitt title="2. Key">
                <SchluesselFeld
                    ctx=ctx
                    name="litellm"
                    label="LiteLLM-Key"
                    hinweis="Geheim wie ein Passwort. Er wird nur im Schlüsselbund dieses Rechners gespeichert."
                />
            </Abschnitt>
            <Abschnitt title="3. Verbindung testen">
                <p class="t-subheadline t-secondary">
                    "Cockpit fragt den Proxy nach seiner Modellliste und meldet, ob er antwortet, den Key annimmt und das Modell kennt. Dabei wird nichts berechnet."
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
                                    "Der Test geht, sobald Adresse und Key gespeichert sind."
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
                            let (ton, text) = pruefung_klartext(&p);
                            let tone = match ton {
                                Ton::Gut => StatusTone::Green,
                                Ton::Offen => StatusTone::Gray,
                                Ton::Problem => StatusTone::Red,
                            };
                            view! {
                                <HStack spacing="8px">
                                    <Status tone=tone>{if ton == Ton::Gut { "Gelungen" } else { "Nicht gelungen" }}</Status>
                                    <span class="t-subheadline">{text}</span>
                                </HStack>
                            }
                        })
                }}
            </Abschnitt>
        </VStack>
    }
}
