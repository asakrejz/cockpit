//! Schritt 5: Zusammenfassung mit Haken und Klartext.

use cockpit_core::einrichten::{Schritt, Stand, Ton, zusammenfassung};
use feature_ui::*;
use leptos::prelude::*;

use super::felder::{Einleitung, Kontext};
use crate::sync_status::SyncStatus;

#[component]
pub fn FertigSchritt(ctx: Kontext, zu_schritt: Callback<Schritt>) -> impl IntoView {
    let zeilen = move || {
        let health = ctx.store.health.get().unwrap_or_default();
        zusammenfassung(&Stand {
            einrichtung: ctx.einr.get(),
            jira: health.anmeldung.jira.clone(),
            github: health.anmeldung.github.clone(),
            endpunkt: health.endpunkt.clone(),
            container: health.container.clone(),
            agent: health.agent.clone(),
            pruefung: ctx.pruefung.get(),
        })
    };
    view! {
        <VStack spacing="12px">
            <Einleitung title="Zusammenfassung">
                <p>
                    "Hier siehst du, was läuft und was noch fehlt. Offene Punkte sind kein Fehler – du kannst sie jederzeit über „Einrichten“ nachholen."
                </p>
            </Einleitung>
            <List>
                <ListSection header="Stand">
                    {move || {
                        zeilen()
                            .into_iter()
                            .map(|z| {
                                let (tone, label, symbol) = match z.ton {
                                    Ton::Gut => (StatusTone::Green, "In Ordnung", "✓ "),
                                    Ton::Offen => (StatusTone::Gray, "Offen", ""),
                                    Ton::Problem => (StatusTone::Red, "Prüfen", "! "),
                                };
                                let schritt = z.schritt;
                                view! {
                                    <ListRow title=format!("{symbol}{}", z.name) subtitle=z.text wrap=true>
                                        <Status tone=tone>{label}</Status>
                                        {(z.ton != Ton::Gut)
                                            .then(|| {
                                                view! {
                                                    <Button
                                                        size=ControlSize::Small
                                                        on_click=move |_| zu_schritt.run(schritt)
                                                    >
                                                        "Ansehen"
                                                    </Button>
                                                }
                                            })}
                                    </ListRow>
                                }
                            })
                            .collect_view()
                    }}
                </ListSection>
            </List>
            <SyncStatus />
            <HStack css="justify-content: flex-end">
                <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href="#/">
                    "Zum Dashboard"
                </Button>
            </HStack>
            <p class="t-footnote t-secondary" style="padding-inline: 4px">
                "Wo Schlüssel liegen: nur im Schlüsselbund dieses Rechners. Sie gehen über die Eingabefelder hier (oder im Terminal mit „cockpit-server token setzen <name>“) hinein und kommen nie in den Browser-Speicher, ins Repo, in die Datenbank oder ins Log. Der echte Agent läuft in Docker oder Podman und spricht mit dem Modell-Endpunkt, den du im Schritt „Modell“ gewählt hast."
            </p>
        </VStack>
    }
}
