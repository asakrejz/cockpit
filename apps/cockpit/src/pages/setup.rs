//! Einrichten: ein geführter Assistent in fünf Schritten – Container, Jira, GitHub, Modellzugang
//! und eine Zusammenfassung. Jeder Schritt lässt sich überspringen und später nachholen.
//!
//! Schlüssel (Client-Secrets, Tokens, der Key des Modell-Endpunkts) kommen in Passwortfelder und gehen einmal
//! an den lokalen Server, der sie in der .env-Datei ablegt. Der Browser speichert sie nirgends
//! (kein `localStorage`, keine Adresse), der Server schickt sie nie zurück. Die Regeln dazu
//! stehen in `cockpit_core::setup`, das Schreiben von `config.toml` im Server.

mod container;
mod felder;
mod fertig;
mod github;
mod jira;
mod modell;

use cockpit_core::setup::Step;
use feature_ui::*;
use leptos::prelude::*;

use crate::store::use_store;
use container::ContainerSchritt;
use felder::Kontext;
use fertig::FertigSchritt;
use github::GitHubSchritt;
use jira::JiraSchritt;
use modell::ModellSchritt;

/// Beschriftungen der Fortschrittsanzeige, in der Reihenfolge von [`Schritt::ALLE`].
const NAMEN: &[&str] = &["Container", "Jira", "GitHub", "Modell", "Fertig"];

#[component]
pub fn SetupPage() -> impl IntoView {
    let store = use_store();
    // Jeder Besuch fragt neu: Zwischen zwei Besuchen kann sich viel getan haben.
    let ctx = Kontext::neu(store);
    let schritt = RwSignal::new(Step::Container);
    let index = Signal::derive(move || schritt.get().index());

    let weiter = move |_| {
        if let Some(n) = schritt.get_untracked().next() {
            schritt.set(n);
        }
    };
    let zurueck = move |_| {
        if let Some(v) = schritt.get_untracked().previous() {
            schritt.set(v);
        }
    };
    let springen = Callback::new(move |s: Step| schritt.set(s));

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
                            "Wir gehen Schritt für Schritt vor. Du brauchst keine Vorkenntnisse, und jeden Schritt kannst du überspringen und später nachholen."
                        </p>
                    </VStack>

                    <Steps labels=NAMEN current=index />

                    {move || match schritt.get() {
                        Step::Container => view! { <ContainerSchritt ctx=ctx /> }.into_any(),
                        Step::Jira => view! { <JiraSchritt ctx=ctx /> }.into_any(),
                        Step::GitHub => view! { <GitHubSchritt ctx=ctx /> }.into_any(),
                        Step::Model => view! { <ModellSchritt ctx=ctx /> }.into_any(),
                        Step::Done => {
                            view! { <FertigSchritt ctx=ctx zu_schritt=springen /> }.into_any()
                        }
                    }}

                    <HStack css="justify-content: space-between; flex-wrap: wrap">
                        <Button
                            disabled=Signal::derive(move || schritt.get().previous().is_none())
                            on_click=zurueck
                        >
                            "Zurück"
                        </Button>
                        {move || {
                            schritt
                                .get()
                                .next()
                                .map(|_| {
                                    view! {
                                        <HStack spacing="12px">
                                            <Button variant=ButtonStyle::Borderless on_click=weiter>
                                                "Überspringen"
                                            </Button>
                                            <Button variant=ButtonStyle::GlassProminent on_click=weiter>
                                                "Weiter"
                                            </Button>
                                        </HStack>
                                    }
                                })
                        }}
                    </HStack>
                </VStack>
            </Container>
        </div>
    }
}
