//! Einrichten: ein geführter Assistent in fünf Schritten – Container, Jira, GitHub, Modellzugang
//! und eine Zusammenfassung. Jeder Schritt lässt sich überspringen und später nachholen.
//!
//! Schlüssel (Client-Secrets, Tokens, der Key des Modell-Endpunkts) kommen in Passwortfelder und gehen einmal
//! an den lokalen Server, der sie im Schlüsselbund ablegt. Der Browser speichert sie nirgends
//! (kein `localStorage`, keine Adresse), der Server schickt sie nie zurück. Die Regeln dazu
//! stehen in `cockpit_core::setup`, das Schreiben von `config.toml` im Server.

mod container;
mod done;
mod fields;
mod github;
mod jira;
mod model;

use cockpit_core::setup::Step;
use feature_ui::*;
use leptos::prelude::*;

use crate::store::use_store;
use container::ContainerStep;
use done::DoneStep;
use fields::Context;
use github::GitHubStep;
use jira::JiraStep;
use model::ModelStep;

/// Beschriftungen der Fortschrittsanzeige, in der Reihenfolge von [`Schritt::ALLE`].
const NAMES: &[&str] = &["Container", "Jira", "GitHub", "Modell", "Fertig"];

#[component]
pub fn SetupPage() -> impl IntoView {
    let store = use_store();
    // Jeder Besuch fragt neu: Zwischen zwei Besuchen kann sich viel getan haben.
    let ctx = Context::create(store);
    let step = RwSignal::new(Step::Container);
    let index = Signal::derive(move || step.get().index());

    let next_step = move |_| {
        if let Some(n) = step.get_untracked().next() {
            step.set(n);
        }
    };
    let back = move |_| {
        if let Some(v) = step.get_untracked().previous() {
            step.set(v);
        }
    };
    let jump = Callback::new(move |s: Step| step.set(s));

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

                    <Steps labels=NAMES current=index />

                    {move || match step.get() {
                        Step::Container => view! { <ContainerStep ctx=ctx /> }.into_any(),
                        Step::Jira => view! { <JiraStep ctx=ctx /> }.into_any(),
                        Step::GitHub => view! { <GitHubStep ctx=ctx /> }.into_any(),
                        Step::Model => view! { <ModelStep ctx=ctx /> }.into_any(),
                        Step::Done => {
                            view! { <DoneStep ctx=ctx to_step=jump /> }.into_any()
                        }
                    }}

                    <HStack css="justify-content: space-between; flex-wrap: wrap">
                        <Button
                            disabled=Signal::derive(move || step.get().previous().is_none())
                            on_click=back
                        >
                            "Zurück"
                        </Button>
                        {move || {
                            step
                                .get()
                                .next()
                                .map(|_| {
                                    view! {
                                        <HStack spacing="12px">
                                            <Button variant=ButtonStyle::Borderless on_click=next_step>
                                                "Überspringen"
                                            </Button>
                                            <Button variant=ButtonStyle::GlassProminent on_click=next_step>
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
