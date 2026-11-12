//! Schritt 1: Läuft Docker oder Podman? Der Agent arbeitet später in einem Container.

use cockpit_core::api::ContainerStatus;
use feature_ui::*;
use leptos::prelude::*;

use super::felder::{Abschnitt, Einleitung, Extern, Kontext};

#[component]
pub fn ContainerSchritt(ctx: Kontext) -> impl IntoView {
    let status = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().map(|h| h.container.clone()))
    };
    let tone = move || match status() {
        Some(ContainerStatus {
            laufzeit: Some(_),
            engine_laeuft: true,
            ..
        }) => StatusTone::Green,
        Some(ContainerStatus {
            laufzeit: Some(_), ..
        }) => StatusTone::Orange,
        _ => StatusTone::Gray,
    };
    let text = move || match status() {
        None => "Prüfe …".to_string(),
        Some(ContainerStatus {
            laufzeit: Some(l),
            engine_laeuft: true,
            ..
        }) => format!("{l} ist installiert und läuft"),
        Some(ContainerStatus {
            laufzeit: Some(l), ..
        }) => format!("{l} ist installiert, läuft aber nicht"),
        Some(_) => "Noch nichts gefunden".to_string(),
    };
    let fehlt = move || status().is_some_and(|s| s.laufzeit.is_none());
    let steht_nicht = move || status().is_some_and(|s| s.laufzeit.is_some() && !s.engine_laeuft);

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
                            .and_then(|s| s.hinweis)
                            .map(|h| view! { <p class="ui-hinweis" role="status">{h}</p> })
                    }}
                    <HStack spacing="12px">
                        <Button on_click=move |_| ctx.laden()>"Erneut prüfen"</Button>
                    </HStack>
                </CardBody>
            </Card>
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
