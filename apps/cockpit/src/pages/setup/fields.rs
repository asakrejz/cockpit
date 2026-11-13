//! Shared building blocks of the setup steps: state, key field, copy field,
//! messages and saving the configuration. The rules (check, normalization) are in
//! `cockpit_core::setup`, writing is in the server; here is only wiring.

use cockpit_core::api::{ConfigChange, ContainerCheck, EndpointCheck, Setup};
use cockpit_core::setup::key_verify;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::wasm_bindgen::JsCast;

use crate::server::{self, CallError};
use crate::store::Store;

/// What all steps share. Everything `Copy` so that it moves into callbacks.
#[derive(Clone, Copy)]
pub struct Context {
    pub store: Store,
    /// The non-secret values from `GET /api/setup`.
    pub setup: RwSignal<Setup>,
    /// Result of the last connection test to the endpoint.
    pub check: RwSignal<Option<EndpointCheck>>,
    /// Result of the check "reachable from the container".
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

    /// Fetches values and sign-in state anew from the server.
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

/// A feedback under a form: `true` = good, `false` = error.
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
                            class="ui-hint"
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

/// Plain text for a failed call.
fn error_text(e: &CallError) -> String {
    match e {
        CallError::Unreachable => {
            "Cockpit antwortet gerade nicht. Läuft das Startfenster noch? Danach „Erneut versuchen“."
                .into()
        }
        CallError::Rejected(m) => m.clone(),
    }
}

/// Writes the change to `config.toml` and reports the result. `then` only runs on success.
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

/// Password field for a key. The value is only in the field: not in the address, not in the
/// memory of the browser, not in the answer. After submitting, the field is empty.
#[component]
pub fn KeyField(
    ctx: Context,
    /// Name in the .env file (`github-oauth`, `atlassian-oauth`, `agent-key`, `jira`, `github`).
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
        // Clear immediately: from here on the value only lives in this one request.
        wert.set(String::new());
        running.set(true);
        back.set(None);
        spawn_local(async move {
            match server::put_key(name, checked).await {
                Ok(_) => {
                    back.set(Some((
                        true,
                        "In der .env-Datei abgelegt. Das Feld ist wieder leer.".into(),
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
                hint=hint.unwrap_or("Wird nur in der .env-Datei im Projektordner dieses Rechners gespeichert.")
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
                    {move || if set() { "Liegt in der .env-Datei" } else { "Noch nichts abgelegt" }}
                </Status>
            </HStack>
            <Message feedback=back />
        </VStack>
    }
}

/// Writes to the clipboard. `false` if the browser does not allow it.
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

/// A value to type off or copy, with a "Kopieren" button.
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
        <div class="ui-copy">
            <span class="t-footnote t-secondary">{label}</span>
            <code class="ui-copy__text">{move || text.get()}</code>
            <Button size=ControlSize::Small on_click=click>"Kopieren"</Button>
        </div>
    }
}

/// An external link that opens in a new tab.
#[component]
pub fn Extern(#[prop(into)] href: String, children: Children) -> impl IntoView {
    view! {
        <a class="ui-link" href=href target="_blank" rel="noopener noreferrer">
            {children()}
        </a>
    }
}

/// Heading and explanation of a step.
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

/// A card with a heading – a section inside a step.
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
