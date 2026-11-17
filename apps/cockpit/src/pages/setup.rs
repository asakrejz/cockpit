//! Setup: a guided assistant in six steps – container, Jira, GitHub, model access,
//! optional live update (webhooks) and a summary. Every step can be skipped and caught up on later.
//!
//! Keys (client secrets, tokens, the key of the model endpoint) go into password fields and go once
//! to the local server, which stores them in the .env file. The browser stores them nowhere
//! (no `localStorage`, no address), the server never sends them back. The rules for this
//! are in `cockpit_core::setup`, writing `config.toml` is in the server.

mod agent_repos;
mod container;
mod done;
mod fields;
mod github;
mod jira;
mod live;
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
use live::LiveStep;
use model::ModelStep;

/// Labels of the progress display, in the order of [`Step::ALL`].
const NAMES: &[&str] = &["Container", "Jira", "GitHub", "Modell", "Live", "Fertig"];

#[component]
pub fn SetupPage() -> impl IntoView {
    let store = use_store();
    // Every visit asks anew: a lot can have happened between two visits.
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
                        Step::Live => view! { <LiveStep ctx=ctx /> }.into_any(),
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
