//! Einrichten: Stand und nächster Schritt je Dienst – Jira, GitHub, LiteLLM und Container.
//!
//! Die Seite hat **keine Eingabefelder für Schlüssel**. Anmeldungen laufen über „Mit … anmelden“
//! (OAuth, siehe `oauth_card`); Client-Secrets und der LiteLLM-Key gehen im Terminal per
//! `cockpit-server token setzen …` direkt in den Schlüsselbund des Backends. Der Browser sieht
//! nur ja/nein aus `GET /api/health` und das Ergebnis des Verbindungstests.

use cockpit_core::api::{ContainerStatus, LitellmPruefung, LitellmStatus};
use cockpit_core::{AuthKind, ServiceStatus};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::oauth_card::{OAuthCard, Service};
use crate::server::{self, CallError};
use crate::store::{Store, use_store};
use crate::sync_status::SyncStatus;

/// Stand des Verbindungstests zum LiteLLM-Proxy.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Test {
    Idle,
    Running,
    Done(LitellmPruefung),
    Failed(String),
}

/// Eine Zeile „Nächste Schritte“: erledigt oder offen, mit Text.
fn step_row(done: bool, title: &'static str, subtitle: &'static str) -> impl IntoView {
    view! {
        <ListRow title=title subtitle=subtitle wrap=true>
            <Status tone=if done { StatusTone::Green } else { StatusTone::Gray }>
                {if done { "Erledigt" } else { "Offen" }}
            </Status>
        </ListRow>
    }
}

fn service_connected(pick: fn(&server::Health) -> &ServiceStatus) -> Signal<(bool, bool)> {
    let store = use_store();
    Signal::derive(move || {
        store.health.with(|h| {
            h.as_ref()
                .map(|h| {
                    let s = pick(h);
                    (s.verbunden || s.oauth_moeglich, s.verbunden)
                })
                .unwrap_or_default()
        })
    })
}

/// Jira oder GitHub: Anmeldung (Knöpfe) und die Schritte dazu.
#[component]
fn ServiceSection(service: Service) -> impl IntoView {
    let store = use_store();
    let (client_id, secret, config) = match service {
        Service::Atlassian => (
            "Client-ID der Atlassian-App in config.toml eintragen",
            "Client-Secret ablegen: cockpit-server token setzen atlassian-oauth (im Terminal, Wert über die Standardeingabe)",
            "[atlassian] client_id = \"…\" – Jira-Projekte, Abfrage und Site stehen optional unter [jira]",
        ),
        Service::GitHub => (
            "Client-ID der GitHub App in config.toml eintragen",
            "Client-Secret ablegen: cockpit-server token setzen github-oauth (im Terminal, Wert über die Standardeingabe)",
            "[github_oauth] client_id = \"…\" – dazu die Repos unter [github] repos = [\"firma/app\"], die der Abgleich braucht",
        ),
    };
    let ready = match service {
        Service::Atlassian => service_connected(|h| &h.anmeldung.jira),
        Service::GitHub => service_connected(|h| &h.anmeldung.github),
    };
    let (title, intro) = match service {
        Service::Atlassian => (
            "Jira",
            "Damit Cockpit deine Tickets und die deines Teams lesen, kommentieren und den Status ändern kann.",
        ),
        Service::GitHub => (
            "GitHub",
            "Für Pull Requests, Reviews und Branches. Die GitHub App bekommt nur Zugriff auf die Repos, die du ihr gibst.",
        ),
    };
    view! {
        <VStack spacing="12px">
            <Intro title=title text=intro />
            <OAuthCard
                service=service
                on_connected=move |_| store.refresh_health()
                on_disconnected=move |_| store.refresh_health()
            />
            <List>
                <ListSection header="Nächste Schritte" footer=config>
                    {move || step_row(ready.get().0, client_id, "Die Client-ID ist nicht geheim.")}
                    {move || step_row(ready.get().0, "Client-Secret", secret)}
                    {move || {
                        step_row(
                            ready.get().1,
                            match service {
                                Service::Atlassian => "Mit Atlassian anmelden",
                                Service::GitHub => "Mit GitHub anmelden",
                            },
                            "Der Knopf oben öffnet die Anmeldung im Browser; „Trennen“ löscht die Tokens wieder.",
                        )
                    }}
                </ListSection>
            </List>
        </VStack>
    }
}

/// Text für den Stand der Anmeldung in der Übersicht.
fn auth_text(s: &ServiceStatus) -> String {
    match (s.verbunden, s.art) {
        (true, AuthKind::OAuth) => "Verbunden per OAuth".into(),
        (true, kind) => format!("Verbunden ({})", kind.label()),
        (false, _) => "Nicht verbunden".into(),
    }
}

/// LiteLLM: Adresse und Modell (aus config.toml), Key (nur ja/nein) und der Verbindungstest.
#[component]
fn LitellmSection() -> impl IntoView {
    let store = use_store();
    let test = RwSignal::new(Test::Idle);
    let status = move || store.health.with(|h| h.as_ref().map(|h| h.litellm.clone()));

    let run = move |_| {
        test.set(Test::Running);
        spawn_local(async move {
            test.set(match server::fetch_litellm_check().await {
                Ok(result) => Test::Done(result),
                Err(CallError::Unreachable) => {
                    Test::Failed("Der Server ist nicht erreichbar".into())
                }
                Err(e) => Test::Failed(e.message()),
            });
        });
    };

    let state_tone = move || match status() {
        Some(LitellmStatus {
            konfiguriert: true,
            key: true,
            modell: Some(_),
            ..
        }) => StatusTone::Green,
        Some(LitellmStatus {
            konfiguriert: true, ..
        }) => StatusTone::Orange,
        _ => StatusTone::Gray,
    };
    let state_text = move || match status() {
        Some(LitellmStatus {
            konfiguriert: true,
            key: true,
            modell: Some(_),
            ..
        }) => "Eingerichtet",
        Some(LitellmStatus {
            konfiguriert: true, ..
        }) => "Unvollständig",
        Some(_) => "Nicht eingerichtet",
        None => "Prüfe …",
    };
    let line = |ok: bool, yes: &'static str, no: &'static str| {
        view! {
            <Status tone=if ok { StatusTone::Green } else { StatusTone::Red }>
                {if ok { yes } else { no }}
            </Status>
        }
    };

    view! {
        <VStack spacing="12px">
            <Intro
                title="Modellzugang (LiteLLM)"
                text="Der Agent spricht mit seinem Modell über einen LiteLLM-Proxy – kein Anthropic-API-Key. Der Key liegt nur im Schlüsselbund des Backends."
            />
            <List>
                <ListSection
                    header="Stand"
                    footer="Adresse und Modellname gehören in config.toml ([litellm] url, modell); der Key nie."
                >
                    <ListRow title="LiteLLM-Proxy">
                        <Status tone=Signal::derive(state_tone)>{state_text}</Status>
                    </ListRow>
                    <ListRow
                        title="Adresse"
                        detail=Signal::derive(move || {
                            status().and_then(|s| s.url).unwrap_or_else(|| "–".into())
                        })
                    />
                    <ListRow
                        title="Modell"
                        detail=Signal::derive(move || {
                            status().and_then(|s| s.modell).unwrap_or_else(|| "–".into())
                        })
                    />
                    <ListRow title="Key im Schlüsselbund">
                        <Status tone=Signal::derive(move || {
                            if status().is_some_and(|s| s.key) { StatusTone::Green } else { StatusTone::Gray }
                        })>
                            {move || if status().is_some_and(|s| s.key) { "Liegt bereit" } else { "Fehlt" }}
                        </Status>
                    </ListRow>
                </ListSection>
            </List>
            <List>
                <ListSection header="Nächste Schritte">
                    {move || {
                        let s = status().unwrap_or_default();
                        step_row(
                            s.konfiguriert && s.modell.is_some(),
                            "Adresse und Modell eintragen",
                            "In config.toml: [litellm] url = \"http://localhost:4000\" und modell = \"claude-sonnet-5-5\" (so heißt das Modell im Proxy). Danach den Server neu starten.",
                        )
                    }}
                    {move || {
                        step_row(
                            status().is_some_and(|s| s.key),
                            "Key ablegen",
                            "Im Terminal: cockpit-server token setzen litellm – den Key über die Standardeingabe einfügen, er landet im Schlüsselbund.",
                        )
                    }}
                    {move || {
                        step_row(
                            matches!(test.get(), Test::Done(ref p) if p.key_ok && p.modell_vorhanden),
                            "Verbindung testen",
                            "Fragt den Proxy nach seiner Modellliste (GET /v1/models) und meldet, ob er antwortet, den Key annimmt und das Modell anbietet.",
                        )
                    }}
                </ListSection>
            </List>
            <HStack spacing="12px">
                <Button
                    disabled=Signal::derive(move || {
                        test.get() == Test::Running || !status().is_some_and(|s| s.konfiguriert)
                    })
                    on_click=run
                >
                    {move || if test.get() == Test::Running { "Teste …" } else { "Verbindung testen" }}
                </Button>
            </HStack>
            {move || match test.get() {
                Test::Done(p) => {
                    view! {
                        <List>
                            <ListSection
                                header="Ergebnis"
                                footer=p.hinweis.clone().unwrap_or_default()
                            >
                                <ListRow title="Proxy erreichbar">
                                    {line(p.erreichbar, "Ja", "Nein")}
                                </ListRow>
                                <ListRow title="Key angenommen">
                                    {line(p.key_ok, "Ja", "Nein")}
                                </ListRow>
                                <ListRow title="Modell vorhanden">
                                    {line(p.modell_vorhanden, "Ja", "Nein")}
                                </ListRow>
                            </ListSection>
                        </List>
                    }
                        .into_any()
                }
                Test::Failed(message) => {
                    view! {
                        <p class="t-footnote t-secondary" role="status" style="padding-inline: 4px">
                            {message}
                        </p>
                    }
                        .into_any()
                }
                _ => ().into_any(),
            }}
        </VStack>
    }
}

/// Container-Laufzeit: welche gefunden wurde, ob die Engine läuft, und was zu tun ist.
#[component]
fn ContainerSection() -> impl IntoView {
    let store = use_store();
    let status = move || {
        store
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
        }) => format!("{l} läuft"),
        Some(ContainerStatus {
            laufzeit: Some(l), ..
        }) => format!("{l} läuft nicht"),
        Some(_) => "Keine Laufzeit".to_string(),
    };
    let origin = move || {
        match status().map(|s| s.quelle).as_deref() {
            Some("umgebung") => "COCKPIT_CONTAINER",
            Some("konfiguration") => "config.toml",
            Some("erkannt") => "automatisch erkannt",
            _ => "–",
        }
        .to_string()
    };
    view! {
        <VStack spacing="12px">
            <Intro
                title="Container-Laufzeit"
                text="Der Agent läuft später in einem Container – mit Docker (Rancher Desktop) oder Podman. Cockpit ruft die Programme nur als Befehl auf, nie den Socket."
            />
            <List>
                <ListSection header="Stand">
                    <ListRow title="Laufzeit">
                        <Status tone=Signal::derive(tone)>{text}</Status>
                    </ListRow>
                    <ListRow title="Gewählt über" detail=Signal::derive(origin) />
                </ListSection>
            </List>
            {move || {
                status()
                    .and_then(|s| s.hinweis)
                    .map(|hinweis| {
                        view! {
                            <p class="t-footnote t-secondary" role="status" style="padding-inline: 4px">
                                {hinweis}
                            </p>
                        }
                    })
            }}
            <List>
                <ListSection
                    header="Nächste Schritte"
                    footer="Das Agent-Image baut Cockpit erst mit dem echten Agenten."
                >
                    {move || {
                        step_row(
                            status().is_some_and(|s| s.laufzeit.is_some()),
                            "Docker oder Podman installieren",
                            "Rancher Desktop mit Engine „dockerd (moby)“, oder Podman (brew install podman).",
                        )
                    }}
                    {move || {
                        step_row(
                            status().is_some_and(|s| s.engine_laeuft),
                            "Engine starten",
                            "Rancher Desktop öffnen bzw. „podman machine start“. Geprüft wird mit „<Laufzeit> info“.",
                        )
                    }}
                    <ListRow
                        title="Laufzeit festlegen (optional)"
                        subtitle="Sonst nimmt Cockpit podman, dann docker. Fest: [agent] container = \"docker\" oder \"podman\" in config.toml, oder die Umgebungsvariable COCKPIT_CONTAINER."
                        wrap=true
                    />
                </ListSection>
            </List>
        </VStack>
    }
}

#[component]
pub fn SetupPage() -> impl IntoView {
    let store: Store = use_store();
    // Jeder Besuch fragt neu: Zwischen zwei Besuchen kann sich viel getan haben.
    store.refresh_health();

    let summary = move || {
        let health = store.health.get();
        let jira = health
            .as_ref()
            .map(|h| auth_text(&h.anmeldung.jira))
            .unwrap_or_else(|| "Prüfe …".into());
        let github = health
            .as_ref()
            .map(|h| auth_text(&h.anmeldung.github))
            .unwrap_or_else(|| "Prüfe …".into());
        (jira, github)
    };
    let tone = |connected: Signal<(bool, bool)>| {
        Signal::derive(move || {
            if connected.get().1 {
                StatusTone::Green
            } else {
                StatusTone::Gray
            }
        })
    };
    let jira_on = service_connected(|h| &h.anmeldung.jira);
    let github_on = service_connected(|h| &h.anmeldung.github);

    view! {
        <div class="ck-setup">
            <Container readable=true>
                <VStack spacing="28px">
                    <VStack spacing="10px">
                        <HStack spacing="10px">
                            <span style="display: inline-flex; color: var(--sys-color-tint)">
                                <Icon path=icons::BRAND size=26 />
                            </span>
                            <span class="t-title3 t-bold">"Cockpit"</span>
                            <Badge variant=BadgeVariant::Neutral>"Lokal"</Badge>
                        </HStack>
                        <h1 class="t-display">"Einrichten."</h1>
                        <p class="t-title3 t-secondary">
                            "Jeder Dienst zeigt seinen Stand und den nächsten Schritt. Schlüssel gibst du hier nie ein."
                        </p>
                    </VStack>

                    <List>
                        <ListSection header="Übersicht" prominent=true>
                            <ListRow title="Jira">
                                <Status tone=tone(jira_on)>{move || summary().0}</Status>
                            </ListRow>
                            <ListRow title="GitHub">
                                <Status tone=tone(github_on)>{move || summary().1}</Status>
                            </ListRow>
                        </ListSection>
                    </List>

                    <ServiceSection service=Service::Atlassian />
                    <ServiceSection service=Service::GitHub />
                    <LitellmSection />
                    <ContainerSection />
                    <SyncStatus />

                    <HStack css="justify-content: flex-end">
                        <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href="#/">
                            "Zum Dashboard"
                        </Button>
                    </HStack>

                    <p class="t-footnote t-secondary" style="padding-inline: 4px">
                        "Wo Schlüssel liegen: nur im Schlüsselbund des Backends. Sie gehen im Terminal mit „cockpit-server token setzen <name>“ hinein (Wert über die Standardeingabe) und kommen nie in den Browser, ins Repo, in die Datenbank oder ins Log. Der Browser sieht nur ja/nein. Der Agent im Server ist noch simuliert; der echte Agent kommt mit Docker oder Podman und dem LiteLLM-Proxy."
                    </p>
                </VStack>
            </Container>
        </div>
    }
}

/// Überschrift und Erklärung eines Abschnitts.
#[component]
fn Intro(title: &'static str, text: &'static str) -> impl IntoView {
    view! {
        <div class="ui-list__section">
            <p class="ui-list__header" data-prominent="">{title}</p>
            <p class="t-subheadline t-secondary" style="padding-inline: 4px">{text}</p>
        </div>
    }
}
