//! Gemeinsame Bausteine der Einrichtungsschritte: Zustand, Schlüsselfeld, Kopierfeld,
//! Meldungen und das Speichern der Konfiguration. Die Regeln (Prüfung, Normalisierung) stehen in
//! `cockpit_core::setup`, das Schreiben im Server; hier ist nur Verdrahtung.

use cockpit_core::api::{ConfigChange, ContainerCheck, EndpointCheck, Setup};
use cockpit_core::setup::key_verify;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::wasm_bindgen::JsCast;

use crate::server::{self, CallError};
use crate::store::Store;

/// Was alle Schritte teilen. Alles `Copy`, damit es in Rückrufe wandert.
#[derive(Clone, Copy)]
pub struct Context {
    pub store: Store,
    /// Die nicht geheimen Werte aus `GET /api/setup`.
    pub setup: RwSignal<Setup>,
    /// Ergebnis des letzten Verbindungstests zum Endpunkt.
    pub check: RwSignal<Option<EndpointCheck>>,
    /// Ergebnis der Prüfung „aus dem Container erreichbar“.
    pub container_check: RwSignal<Option<ContainerCheck>>,
}

impl Context {
    pub fn create(store: Store) -> Self {
        let k = Self {
            store,
            setup: RwSignal::new(Setup::default()),
            check: RwSignal::new(None),
            container_check: RwSignal::new(None),
        };
        k.laden();
        k
    }

    /// Holt Werte und Anmeldestand neu vom Server.
    pub fn laden(&self) {
        let k = *self;
        k.store.refresh_health();
        spawn_local(async move {
            if let Ok(e) = server::fetch_setup().await {
                k.setup.set(e);
            }
        });
    }
}

/// Eine Rückmeldung unter einem Formular: `true` = gut, `false` = Fehler.
pub type Feedback = RwSignal<Option<(bool, String)>>;

#[component]
pub fn Message(feedback: Feedback) -> impl IntoView {
    view! {
        {move || {
            feedback
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
fn error_text(e: &CallError) -> String {
    match e {
        CallError::Unreachable => {
            "Cockpit antwortet gerade nicht. Läuft das Startfenster noch? Danach „Erneut versuchen“."
                .into()
        }
        CallError::Rejected(m) => m.clone(),
    }
}

/// Schreibt die Änderung in `config.toml` und meldet das Ergebnis. `danach` läuft nur bei Erfolg.
pub fn config_save(
    ctx: Context,
    change: ConfigChange,
    feedback: Feedback,
    success: &'static str,
    afterwards: impl Fn() + 'static,
) {
    feedback.set(None);
    spawn_local(async move {
        match server::put_config(&change).await {
            Ok(answer) => {
                let mut text = success.to_string();
                if !answer.takes_effect_immediately {
                    text.push_str(" Starte Cockpit neu, damit die Änderung gilt.");
                }
                if let Some(h) = answer.hint {
                    text.push(' ');
                    text.push_str(&h);
                }
                feedback.set(Some((true, text)));
                ctx.laden();
                afterwards();
            }
            Err(e) => feedback.set(Some((false, error_text(&e)))),
        }
    });
}

/// Passwortfeld für einen Schlüssel. Der Wert steht nur im Feld: nicht in der Adresse, nicht im
/// Speicher des Browsers, nicht in der Antwort. Nach dem Absenden ist das Feld leer.
#[component]
pub fn KeyField(
    ctx: Context,
    /// Name im Schlüsselbund (`github-oauth`, `atlassian-oauth`, `agent-key`, `jira`, `github`).
    name: &'static str,
    label: &'static str,
    #[prop(optional)] hint: Option<&'static str>,
) -> impl IntoView {
    let wert = RwSignal::new(String::new());
    let back: Feedback = RwSignal::new(None);
    let running = RwSignal::new(false);
    let set = move || ctx.setup.with(|e| e.has_key(name));

    let send = move |_| {
        let checked = match key_verify(&wert.get_untracked()) {
            Ok(w) => w,
            Err(m) => {
                back.set(Some((false, m)));
                return;
            }
        };
        // Sofort leeren: Der Wert lebt ab hier nur noch in dieser einen Anfrage.
        wert.set(String::new());
        running.set(true);
        back.set(None);
        spawn_local(async move {
            match server::put_key(name, checked).await {
                Ok(_) => {
                    back.set(Some((
                        true,
                        "Im Schlüsselbund abgelegt. Das Feld ist wieder leer.".into(),
                    )));
                    ctx.laden();
                }
                Err(e) => back.set(Some((false, error_text(&e)))),
            }
            running.set(false);
        });
    };

    view! {
        <VStack spacing="8px">
            <TextField
                label=label
                value=wert
                input_type="password"
                autocomplete="off"
                hint=hint.unwrap_or("Wird nur im Schlüsselbund dieses Rechners gespeichert.")
            />
            <HStack spacing="12px">
                <Button
                    variant=ButtonStyle::GlassProminent
                    disabled=Signal::derive(move || running.get() || wert.with(|w| w.trim().is_empty()))
                    on_click=send
                >
                    "Sicher ablegen"
                </Button>
                <Status tone=Signal::derive(move || {
                    if set() { StatusTone::Green } else { StatusTone::Gray }
                })>
                    {move || if set() { "Liegt im Schlüsselbund" } else { "Noch nichts abgelegt" }}
                </Status>
            </HStack>
            <Message feedback=back />
        </VStack>
    }
}

/// Schreibt in die Zwischenablage. `false`, wenn der Browser es nicht erlaubt.
fn copy(text: &str) -> bool {
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
pub fn CopyField(
    store: Store,
    #[prop(into)] label: String,
    #[prop(into)] text: Signal<String>,
) -> impl IntoView {
    let click = move |_| {
        if copy(&text.get_untracked()) {
            store.notify("Kopiert");
        } else {
            store.notify("Kopieren ging nicht – markiere den Text und drücke Cmd+C.");
        }
    };
    view! {
        <div class="ui-kopie">
            <span class="t-footnote t-secondary">{label}</span>
            <code class="ui-kopie__text">{move || text.get()}</code>
            <Button size=ControlSize::Small on_click=click>"Kopieren"</Button>
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
pub fn Intro(title: &'static str, children: Children) -> impl IntoView {
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
pub fn Section(#[prop(into)] title: String, children: Children) -> impl IntoView {
    view! {
        <Card>
            <CardBody css="gap: 12px">
                <span class="t-headline">{title}</span>
                {children()}
            </CardBody>
        </Card>
    }
}
