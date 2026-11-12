//! Schritt 1: Läuft Docker oder Podman? Der Agent arbeitet später in einem Container.

use std::time::Duration;

use cockpit_core::api::{ContainerStatus, ImageBau};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::felder::{Abschnitt, Einleitung, Extern, Kontext};
use crate::server::{self, CallError};
use crate::store::Store;

/// Fragt den Stand des Image-Baus ab und wiederholt das, solange er läuft.
fn bau_beobachten(bau: RwSignal<Option<ImageBau>>, store: Store) {
    spawn_local(async move {
        if let Ok(b) = server::fetch_image().await {
            let weiter = b.running;
            if bau.try_set(Some(b)).is_some() {
                return;
            }
            if weiter {
                set_timeout(
                    move || bau_beobachten(bau, store),
                    Duration::from_millis(1500),
                );
            } else {
                store.refresh_health();
            }
        }
    });
}

#[component]
pub fn ContainerSchritt(ctx: Kontext) -> impl IntoView {
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
    let bau = RwSignal::new(None::<ImageBau>);
    let bau_fehler = RwSignal::new(None::<String>);
    bau_beobachten(bau, ctx.store);
    let bauen = move |_| {
        bau_fehler.set(None);
        spawn_local(async move {
            match server::post_image_bauen().await {
                Ok(b) => {
                    bau.set(Some(b));
                    bau_beobachten(bau, ctx.store);
                }
                Err(CallError::Unreachable) => bau_fehler.set(Some(
                    "Cockpit antwortet gerade nicht. Läuft das Startfenster noch?".into(),
                )),
                Err(e) => bau_fehler.set(Some(e.message())),
            }
        });
    };
    let bau_laeuft = move || bau.with(|b| b.as_ref().is_some_and(|b| b.running));
    let image_da = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().is_some_and(|h| h.agent.image))
    };
    let engine_laeuft = move || status().is_some_and(|s| s.runtime.is_some() && s.engine_running);
    let fehlt = move || status().is_some_and(|s| s.runtime.is_none());
    let steht_nicht = move || status().is_some_and(|s| s.runtime.is_some() && !s.engine_running);

    view! {
        <VStack spacing="12px">
            <Einleitung title="Container-Programm">
                <p>
                    "Cockpit lässt seinen Agenten später in einem „Container“ arbeiten – das ist ein abgeschlossener Arbeitsraum auf deinem Rechner. So kann der Agent Code ausprobieren, ohne an deinen Dateien etwas zu verändern."
                </p>
                <p>
                    "Dafür braucht es ein kostenloses Programm, das solche Container startet. Du brauchst es erst, wenn der Agent wirklich arbeitet – du kannst diesen Schritt also auch überspringen."
                </p>
            </Einleitung>
            <Card>
                <CardBody css="gap: 12px">
                    <HStack css="justify-content: space-between">
                        <span class="t-headline">"Stand auf diesem Rechner"</span>
                        <Status tone=Signal::derive(tone)>{text}</Status>
                    </HStack>
                    {move || {
                        status()
                            .and_then(|s| s.hint)
                            .map(|h| view! { <p class="ui-hinweis" role="status">{h}</p> })
                    }}
                    <HStack spacing="12px">
                        <Button on_click=move |_| ctx.laden()>"Erneut prüfen"</Button>
                    </HStack>
                </CardBody>
            </Card>
            {move || {
                engine_laeuft()
                    .then(|| {
                        view! {
                            <Abschnitt title="Agent-Image">
                                <p class="t-subheadline t-secondary">
                                    "Der Agent arbeitet in einem Container aus einem eigenen „Image“. Das baust du einmal hier – es lädt dabei Programme aus dem Internet und dauert einige Minuten. Bei einer neuen Cockpit-Version baust du es erneut."
                                </p>
                                <HStack spacing="12px">
                                    <Status tone=Signal::derive(move || {
                                        if image_da() { StatusTone::Green } else { StatusTone::Orange }
                                    })>
                                        {move || {
                                            if bau_laeuft() {
                                                "Wird gebaut …"
                                            } else if image_da() {
                                                "Image ist gebaut"
                                            } else {
                                                "Image fehlt noch"
                                            }
                                        }}
                                    </Status>
                                    <Button
                                        variant=ButtonStyle::GlassProminent
                                        disabled=Signal::derive(bau_laeuft)
                                        on_click=bauen
                                    >
                                        {move || if image_da() { "Neu bauen" } else { "Image bauen" }}
                                    </Button>
                                </HStack>
                                {move || {
                                    bau_fehler
                                        .get()
                                        .map(|m| view! { <p class="ui-hinweis" role="alert">{m}</p> })
                                }}
                                {move || {
                                    bau.get()
                                        .and_then(|b| b.error)
                                        .map(|m| view! { <p class="ui-hinweis" role="alert">{m}</p> })
                                }}
                                {move || {
                                    bau.get()
                                        .filter(|b| !b.lines.is_empty())
                                        .map(|b| {
                                            let text = b.lines.join("\n");
                                            view! { <Log text=Signal::derive(move || text.clone()) /> }
                                        })
                                }}
                            </Abschnitt>
                        }
                    })
            }}
            {move || {
                steht_nicht()
                    .then(|| {
                        view! {
                            <Abschnitt title="Das Programm ist da, aber nicht gestartet">
                                <ol class="ui-anleitung">
                                    <li>
                                        "Öffne „Rancher Desktop“ (oder Podman Desktop) aus dem Programme-Ordner und warte, bis oben „läuft“ oder ein grüner Punkt steht – das kann eine Minute dauern."
                                    </li>
                                    <li>
                                        "Bei Podman im Terminal: " <code>"podman machine start"</code>
                                    </li>
                                    <li>"Klicke danach hier auf „Erneut prüfen“."</li>
                                </ol>
                            </Abschnitt>
                        }
                    })
            }}
            {move || {
                fehlt()
                    .then(|| {
                        view! {
                            <Abschnitt title="Option A: Rancher Desktop (empfohlen für Einsteiger)">
                                <ol class="ui-anleitung">
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
                            </Abschnitt>
                            <Abschnitt title="Option B: Podman">
                                <ol class="ui-anleitung">
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
                            </Abschnitt>
                        }
                    })
            }}
        </VStack>
    }
}
