//! Gemeinsame Bausteine der Einrichtungsschritte: Zustand, Schlüsselfeld, Kopierfeld,
//! Meldungen und das Speichern der Konfiguration. Die Regeln (Prüfung, Normalisierung) stehen in
//! `cockpit_core::einrichten`, das Schreiben im Server; hier ist nur Verdrahtung.

use cockpit_core::api::{Einrichtung, KonfigAenderung, LitellmPruefung};
use cockpit_core::einrichten::schluessel_pruefen;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::wasm_bindgen::JsCast;

use crate::server::{self, CallError};
use crate::store::Store;

/// Was alle Schritte teilen. Alles `Copy`, damit es in Rückrufe wandert.
#[derive(Clone, Copy)]
pub struct Kontext {
    pub store: Store,
    /// Die nicht geheimen Werte aus `GET /api/einrichtung`.
    pub einr: RwSignal<Einrichtung>,
    /// Ergebnis des letzten Verbindungstests zum LiteLLM-Proxy.
    pub pruefung: RwSignal<Option<LitellmPruefung>>,
}

impl Kontext {
    pub fn neu(store: Store) -> Self {
        let k = Self {
            store,
            einr: RwSignal::new(Einrichtung::default()),
            pruefung: RwSignal::new(None),
        };
        k.laden();
        k
    }

    /// Holt Werte und Anmeldestand neu vom Server.
    pub fn laden(&self) {
        let k = *self;
        k.store.refresh_health();
        spawn_local(async move {
            if let Ok(e) = server::fetch_einrichtung().await {
                k.einr.set(e);
            }
        });
    }
}

/// Eine Rückmeldung unter einem Formular: `true` = gut, `false` = Fehler.
pub type Rueckmeldung = RwSignal<Option<(bool, String)>>;

#[component]
pub fn Meldung(rueckmeldung: Rueckmeldung) -> impl IntoView {
    view! {
        {move || {
            rueckmeldung
                .get()
                .map(|(gut, text)| {
                    view! {
                        <p
                            class="ui-hinweis"
                            role=if gut { "status" } else { "alert" }
                            style=(!gut).then_some("color: var(--sys-color-red)")
                        >
                            {text}
                        </p>
                    }
                })
        }}
    }
}

/// Klartext für einen gescheiterten Aufruf.
fn fehlertext(e: &CallError) -> String {
    match e {
        CallError::Unreachable => {
            "Cockpit antwortet gerade nicht. Läuft das Startfenster noch? Danach „Erneut versuchen“."
                .into()
        }
        CallError::Rejected(m) => m.clone(),
    }
}

/// Schreibt die Änderung in `config.toml` und meldet das Ergebnis. `danach` läuft nur bei Erfolg.
pub fn konfig_speichern(
    ctx: Kontext,
    aenderung: KonfigAenderung,
    rueckmeldung: Rueckmeldung,
    erfolg: &'static str,
    danach: impl Fn() + 'static,
) {
    rueckmeldung.set(None);
    spawn_local(async move {
        match server::put_konfiguration(&aenderung).await {
            Ok(antwort) => {
                let mut text = erfolg.to_string();
                if !antwort.wirkt_sofort {
                    text.push_str(" Starte Cockpit neu, damit die Änderung gilt.");
                }
                if let Some(h) = antwort.hinweis {
                    text.push(' ');
                    text.push_str(&h);
                }
                rueckmeldung.set(Some((true, text)));
                ctx.laden();
                danach();
            }
            Err(e) => rueckmeldung.set(Some((false, fehlertext(&e)))),
        }
    });
}

/// Passwortfeld für einen Schlüssel. Der Wert steht nur im Feld: nicht in der Adresse, nicht im
/// Speicher des Browsers, nicht in der Antwort. Nach dem Absenden ist das Feld leer.
#[component]
pub fn SchluesselFeld(
    ctx: Kontext,
    /// Name im Schlüsselbund (`github-oauth`, `atlassian-oauth`, `litellm`, `jira`, `github`).
    name: &'static str,
    label: &'static str,
    #[prop(optional)] hinweis: Option<&'static str>,
) -> impl IntoView {
    let wert = RwSignal::new(String::new());
    let rueck: Rueckmeldung = RwSignal::new(None);
    let laeuft = RwSignal::new(false);
    let gesetzt = move || ctx.einr.with(|e| e.hat_schluessel(name));

    let senden = move |_| {
        let geprueft = match schluessel_pruefen(&wert.get_untracked()) {
            Ok(w) => w,
            Err(m) => {
                rueck.set(Some((false, m)));
                return;
            }
        };
        // Sofort leeren: Der Wert lebt ab hier nur noch in dieser einen Anfrage.
        wert.set(String::new());
        laeuft.set(true);
        rueck.set(None);
        spawn_local(async move {
            match server::put_schluessel(name, geprueft).await {
                Ok(_) => {
                    rueck.set(Some((
                        true,
                        "Im Schlüsselbund abgelegt. Das Feld ist wieder leer.".into(),
                    )));
                    ctx.laden();
                }
                Err(e) => rueck.set(Some((false, fehlertext(&e)))),
            }
            laeuft.set(false);
        });
    };

    view! {
        <VStack spacing="8px">
            <TextField
                label=label
                value=wert
                input_type="password"
                autocomplete="off"
                hint=hinweis.unwrap_or("Wird nur im Schlüsselbund dieses Rechners gespeichert.")
            />
            <HStack spacing="12px">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=Signal::derive(move || laeuft.get() || wert.with(|w| w.trim().is_empty()))
                    on_click=senden
                >
                    "Sicher ablegen"
                </Button>
                <Status tone=Signal::derive(move || {
                    if gesetzt() { StatusTone::Green } else { StatusTone::Gray }
                })>
                    {move || if gesetzt() { "Liegt im Schlüsselbund" } else { "Noch nichts abgelegt" }}
                </Status>
            </HStack>
            <Meldung rueckmeldung=rueck />
        </VStack>
    }
}

/// Schreibt in die Zwischenablage. `false`, wenn der Browser es nicht erlaubt.
fn kopieren(text: &str) -> bool {
    let clipboard = js_sys::Reflect::get(&window().navigator(), &"clipboard".into())
        .ok()
        .filter(|c| !c.is_undefined() && !c.is_null());
    let Some(clipboard) = clipboard else {
        return false;
    };
    js_sys::Reflect::get(&clipboard, &"writeText".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .is_some_and(|f| f.call1(&clipboard, &text.into()).is_ok())
}

/// Ein Wert zum Abtippen oder Kopieren, mit „Kopieren“-Knopf.
#[component]
pub fn Kopierfeld(
    store: Store,
    #[prop(into)] label: String,
    #[prop(into)] text: Signal<String>,
) -> impl IntoView {
    let klick = move |_| {
        if kopieren(&text.get_untracked()) {
            store.notify("Kopiert");
        } else {
            store.notify("Kopieren ging nicht – markiere den Text und drücke Cmd+C.");
        }
    };
    view! {
        <div class="ui-kopie">
            <span class="t-footnote t-secondary">{label}</span>
            <code class="ui-kopie__text">{move || text.get()}</code>
            <Button size=ControlSize::Small on_click=klick>"Kopieren"</Button>
        </div>
    }
}

/// Ein externer Link, der in einem neuen Tab aufgeht.
#[component]
pub fn Extern(#[prop(into)] href: String, children: Children) -> impl IntoView {
    view! {
        <a class="ui-link" href=href target="_blank" rel="noopener noreferrer">
            {children()}
        </a>
    }
}

/// Überschrift und Erklärung eines Schritts.
#[component]
pub fn Einleitung(title: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="ui-list__section">
            <p class="ui-list__header" data-prominent="">{title}</p>
            <div class="t-subheadline t-secondary" style="padding-inline: 4px; display: flex; flex-direction: column; gap: 8px">
                {children()}
            </div>
        </div>
    }
}

/// Eine Karte mit Überschrift – ein Abschnitt innerhalb eines Schritts.
#[component]
pub fn Abschnitt(#[prop(into)] title: String, children: Children) -> impl IntoView {
    view! {
        <Card>
            <CardBody css="gap: 12px">
                <span class="t-headline">{title}</span>
                {children()}
            </CardBody>
        </Card>
    }
}
