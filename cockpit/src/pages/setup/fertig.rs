//! Schritt 5: Zusammenfassung mit Haken und Klartext.

use cockpit_core::AuthKind;
use cockpit_core::einrichten::{Schritt, Stand, Ton, zusammenfassung};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::felder::{Einleitung, Kontext};
use crate::server;
use crate::sync_status::SyncStatus;

/// „Token erneuern“ und „Trennen“ für Jira. Trennen löscht den Token im Schlüsselbund (bei
/// OAuth: widerruft die Anmeldung); die Einstellungen bleiben, der Weg zurück ist der Schritt „Jira“.
#[component]
fn JiraAktionen(ctx: Kontext, zu_schritt: Callback<Schritt>) -> impl IntoView {
    let art = move || {
        ctx.store
            .health
            .with(|h| h.as_ref().map(|h| h.anmeldung.jira.art))
    };
    let trennen = move |_| {
        let oauth = art() == Some(AuthKind::OAuth);
        spawn_local(async move {
            let ergebnis = if oauth {
                server::post_revoke("atlassian").await.map(|_| ())
            } else {
                server::delete_schluessel("jira").await.map(|_| ())
            };
            match ergebnis {
                Ok(()) => ctx
                    .store
                    .notify("Jira getrennt – der Zugang wurde gelöscht."),
                Err(e) => ctx.store.notify(e.message()),
            }
            ctx.laden();
        });
    };
    view! {
        {move || {
            (art() == Some(AuthKind::Token))
                .then(|| {
                    view! {
                        <Button
                            size=ControlSize::Small
                            on_click=move |_| zu_schritt.run(Schritt::Jira)
                        >
                            "Token erneuern"
                        </Button>
                    }
                })
        }}
        <Button size=ControlSize::Small destructive=true on_click=trennen>
            "Trennen"
        </Button>
    }
}

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
                                let jira_verbunden = z.name == "Jira" && z.ton == Ton::Gut;
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
                                        {jira_verbunden
                                            .then(|| view! { <JiraAktionen ctx=ctx zu_schritt=zu_schritt /> })}
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
