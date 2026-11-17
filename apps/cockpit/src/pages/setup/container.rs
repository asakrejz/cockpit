//! Step 1: is Docker or Podman running? The agent later works in a container.

use std::time::Duration;

use cockpit_core::api::{ContainerStatus, ImageBuild, ImageState};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::build_log::CollapsibleLog;
use super::fields::{Context, Extern, Intro, Section};
use crate::server::{self, CallError};
use crate::store::Store;

/// Asks for the state of the image build and repeats that as long as it runs.
fn build_watch(build: RwSignal<Option<ImageBuild>>, store: Store) {
    spawn_local(async move {
        if let Ok(b) = server::fetch_image().await {
            let next_step = b.running;
            if build.try_set(Some(b)).is_some() {
                return;
            }
            if next_step {
                set_timeout(
                    move || build_watch(build, store),
                    Duration::from_millis(1500),
                );
            } else {
                store.refresh_health();
            }
        }
    });
}

#[component]
pub fn ContainerStep(ctx: Context) -> impl IntoView {
    let status = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().map(|h| h.container.clone()))
    };
    let tone = move || match status() {
        Some(ContainerStatus {
            runtime: Some(_),
            engine_running: true,
            ..
        }) => StatusTone::Green,
        Some(ContainerStatus {
            runtime: Some(_), ..
        }) => StatusTone::Orange,
        _ => StatusTone::Gray,
    };
    let text = move || match status() {
        None => "Prüfe …".to_string(),
        Some(ContainerStatus {
            runtime: Some(l),
            engine_running: true,
            ..
        }) => format!("{l} ist installiert und läuft"),
        Some(ContainerStatus {
            runtime: Some(l), ..
        }) => format!("{l} ist installiert, läuft aber nicht"),
        Some(_) => "Noch nichts gefunden".to_string(),
    };
    let build = RwSignal::new(None::<ImageBuild>);
    let build_error = RwSignal::new(None::<String>);
    build_watch(build, ctx.store);
    // Per build: collapsed at the start; an error opens it once (the reader can close it again).
    let log_open = RwSignal::new(false);
    let opened_by_error = StoredValue::new(false);
    let has_log =
        Memo::new(move |_| build.with(|b| b.as_ref().is_some_and(|b| !b.lines.is_empty())));
    let log_lines = Signal::derive(move || {
        build.with(|b| b.as_ref().map(|b| b.lines.clone()).unwrap_or_default())
    });
    Effect::new(move |_| {
        let failed = build.with(|b| b.as_ref().is_some_and(|b| b.error.is_some()));
        if failed && !opened_by_error.get_value() {
            opened_by_error.set_value(true);
            log_open.set(true);
        }
    });
    let start_build = move |_| {
        log_open.set(false);
        opened_by_error.set_value(false);
        build_error.set(None);
        spawn_local(async move {
            match server::post_image_build().await {
                Ok(b) => {
                    build.set(Some(b));
                    build_watch(build, ctx.store);
                }
                Err(CallError::Unreachable) => build_error.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => build_error.set(Some(e.message())),
            }
        });
    };
    let build_running = move || build.with(|b| b.as_ref().is_some_and(|b| b.running));
    let image_state = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().map(|h| h.agent.image_state).unwrap_or_default())
    };
    let image_hint = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().and_then(|h| h.agent.image_hint.clone()))
    };
    let image_da = move || image_state() != ImageState::Missing;
    // Memos: the sections below only rebuild when the answer changes, not at every refresh.
    let engine_running =
        Memo::new(move |_| status().is_some_and(|s| s.runtime.is_some() && s.engine_running));
    let missing = Memo::new(move |_| status().is_some_and(|s| s.runtime.is_none()));
    let not_running =
        Memo::new(move |_| status().is_some_and(|s| s.runtime.is_some() && !s.engine_running));

    view! {
        <VStack spacing="12px">
            <Intro title="Container-Programm">
                <p>
                    "Cockpit lässt seinen Agenten später in einem „Container“ arbeiten – das ist ein abgeschlossener Arbeitsraum auf deinem Rechner. So kann der Agent Code ausprobieren, ohne an deinen Dateien etwas zu verändern."
                </p>
                <p>
                    "Dafür braucht es ein kostenloses Programm, das solche Container startet. Du brauchst es erst, wenn der Agent wirklich arbeitet – du kannst diesen Schritt also auch überspringen."
                </p>
            </Intro>
            <Card>
                <CardBody css="gap: 12px">
                    <HStack css="justify-content: space-between">
                        <span class="t-headline">"Stand auf diesem Rechner"</span>
                        <Status tone=Signal::derive(tone)>{text}</Status>
                    </HStack>
                    {move || {
                        status()
                            .and_then(|s| s.hint)
                            .map(|h| view! { <p class="ui-hint" role="status">{h}</p> })
                    }}
                    <HStack spacing="12px">
                        <Button on_click=move |_| ctx.reload()>"Erneut prüfen"</Button>
                    </HStack>
                </CardBody>
            </Card>
            {move || {
                engine_running
                    .get()
                    .then(|| {
                        view! {
                            <Section title="Agent-Image">
                                <p class="t-subheadline t-secondary">
                                    "Der Agent arbeitet in einem Container aus einem eigenen „Image“. Das baust du einmal hier – es lädt dabei Programme aus dem Internet und dauert einige Minuten. Ändert sich die Cockpit-Version, erkennt Cockpit von selbst, dass das Image veraltet ist, und bittet dich, es neu zu bauen."
                                </p>
                                <HStack spacing="12px">
                                    <Status tone=Signal::derive(move || {
                                        if image_state() == ImageState::Current {
                                            StatusTone::Green
                                        } else {
                                            StatusTone::Orange
                                        }
                                    })>
                                        {move || {
                                            if build_running() {
                                                "Wird gebaut …"
                                            } else {
                                                match image_state() {
                                                    ImageState::Current => "Image ist aktuell",
                                                    ImageState::Outdated => "Image ist veraltet",
                                                    ImageState::Unknown => "Image nicht prüfbar",
                                                    ImageState::Missing => "Image fehlt noch",
                                                }
                                            }
                                        }}
                                    </Status>
                                    <Button
                                        variant=ButtonStyle::GlassProminent
                                        disabled=Signal::derive(build_running)
                                        on_click=start_build
                                    >
                                        {move || if image_da() { "Neu bauen" } else { "Image bauen" }}
                                    </Button>
                                </HStack>
                                {move || {
                                    (image_state() == ImageState::Outdated && !build_running())
                                        .then(|| {
                                            view! {
                                                <p class="ui-hint" role="status">
                                                    "Das Image passt nicht zu dieser Cockpit-Version. Bitte neu bauen; bis dahin startet der Agent nicht."
                                                </p>
                                            }
                                        })
                                }}
                                {move || {
                                    (image_state() == ImageState::Unknown)
                                        .then(image_hint)
                                        .flatten()
                                        .map(|m| view! { <p class="ui-hint" role="status">{m}</p> })
                                }}
                                {move || {
                                    build_error
                                        .get()
                                        .map(|m| view! { <p class="ui-hint" role="alert">{m}</p> })
                                }}
                                {move || {
                                    build.get()
                                        .and_then(|b| b.error)
                                        .map(|m| view! { <p class="ui-hint" role="alert">{m}</p> })
                                }}
                                {move || {
                                    has_log
                                        .get()
                                        .then(|| {
                                            view! { <CollapsibleLog lines=log_lines expanded=log_open /> }
                                        })
                                }}
                            </Section>
                        }
                    })
            }}
            {move || {
                not_running
                    .get()
                    .then(|| {
                        view! {
                            <Section title="Das Programm ist da, aber nicht gestartet">
                                <ol class="ui-instructions">
                                    <li>
                                        "Öffne „Rancher Desktop“ (oder Podman Desktop) aus dem Programme-Ordner und warte, bis oben „läuft“ oder ein grüner Punkt steht – das kann eine Minute dauern."
                                    </li>
                                    <li>
                                        "Bei Podman im Terminal: " <code>"podman machine start"</code>
                                    </li>
                                    <li>"Klicke danach hier auf „Erneut prüfen“."</li>
                                </ol>
                            </Section>
                        }
                    })
            }}
            {move || {
                missing
                    .get()
                    .then(|| {
                        view! {
                            <Section title="Option A: Rancher Desktop (empfohlen für Einsteiger)">
                                <ol class="ui-instructions">
                                    <li>
                                        "Lade das Programm herunter: "
                                        <Extern href="https://rancherdesktop.io/">"rancherdesktop.io"</Extern>
                                        " (rund 500 MB, der Download dauert ein paar Minuten)."
                                    </li>
                                    <li>"Öffne die heruntergeladene Datei und ziehe das Programm in den Ordner „Programme“."</li>
                                    <li>
                                        "Starte „Rancher Desktop“. Beim ersten Start fragt es nach der Container-Engine: Wähle "
                                        <strong>"dockerd (moby)"</strong>
                                        ". Die Frage nach Kubernetes brauchst du nicht – du kannst es abschalten."
                                    </li>
                                    <li>
                                        "Warte, bis Rancher Desktop fertig gestartet ist, und klicke oben auf „Erneut prüfen“."
                                    </li>
                                </ol>
                            </Section>
                            <Section title="Option B: Podman">
                                <ol class="ui-instructions">
                                    <li>
                                        "Installiere Podman nach der Anleitung auf "
                                        <Extern href="https://podman.io/docs/installation">
                                            "podman.io"
                                        </Extern>
                                        " – oder im Terminal mit " <code>"brew install podman"</code> "."
                                    </li>
                                    <li>
                                        "Richte die kleine Hilfs-Maschine ein und starte sie: "
                                        <code>"podman machine init"</code> " und danach "
                                        <code>"podman machine start"</code> "."
                                    </li>
                                    <li>"Klicke danach hier auf „Erneut prüfen“."</li>
                                </ol>
                            </Section>
                        }
                    })
            }}
        </VStack>
    }
}
