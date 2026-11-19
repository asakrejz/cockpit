use feature_ui::*;
use leptos::prelude::*;

use super::{
    Story,
    common::{Section, Token},
};

pub const STORY: Story = Story {
    id: "stack-nav",
    title: "Ebenen (StackNav)",
    group: "Navigation",
    description: "Schmal arbeitet eine Seite mit Liste und Detail wie eine iOS-Navigation: immer nur eine Ebene ist sichtbar, das Detail schiebt von rechts herein, oben links schwebt ein runder Glas-Knopf „‹“ (Zurück) über dem Inhalt. Ab 768 px stehen die Spalten nebeneinander.",
    api: "<SplitView level back_label back_href> · StackLevel",
    render,
};

/// Address of the demo page; the entry number follows after a slash.
const DEMO: &str = "#stack-nav-demo";

fn render() -> AnyView {
    view! {
        <Section title="Zum Ausprobieren" api="375 × 640 · Tippe auf einen Eintrag, dann auf „‹ Liste“">
            <iframe class="pg-stack-frame" src=format!("./{DEMO}") title="Beispiel mit zwei Ebenen"></iframe>
        </Section>
        <Section title="So wird sie benutzt">
            <VStack spacing="8px">
                <Text variant=TextStyle::Body>
                    "Die Ebene kommt aus der Adresse der Seite, nicht aus einem eigenen Schalter. So funktionieren Zurück-Taste des Browsers und Neuladen."
                </Text>
                <Token>"<SplitView level=Signal::derive(|| route.level()) back_label=\"Posteingang\" back_href=\"#/\">"</Token>
                <Text variant=TextStyle::Subheadline color=TextColor::Secondary>
                    "Die erste Spalte ist die Liste, alle weiteren gehören zum Detail. Auch ein Wisch vom linken Rand führt zurück. Bei „Bewegung reduzieren“ springt die Ansicht ohne Übergang."
                </Text>
            </VStack>
        </Section>
    }
    .into_any()
}

/// The page inside the frame: a real `SplitView` in a narrow window.
#[component]
pub fn Demo() -> impl IntoView {
    let entry = RwSignal::new(read_hash());
    let listener = window_event_listener(leptos::ev::hashchange, move |_| entry.set(read_hash()));
    on_cleanup(move || listener.remove());
    let level = Signal::derive(move || {
        if entry.get().is_some() {
            StackLevel::Detail
        } else {
            StackLevel::List
        }
    });
    view! {
        <SplitView level=level back_label="Liste" back_href=DEMO>
            <Pane label="Liste">
                <PaneHeader>
                    <h1 class="t-large-title t-bold">"Liste"</h1>
                </PaneHeader>
                <ul class="ui-sidebar__nav">
                    {(1..=6)
                        .map(|n| {
                            view! {
                                <SidebarItem href=format!("{DEMO}/{n}") label=format!("Eintrag {n}") />
                            }
                        })
                        .collect_view()}
                </ul>
            </Pane>
            <Pane label="Detail">
                <PaneHeader>
                    <h1 class="t-large-title t-bold">
                        {move || format!("Eintrag {}", entry.get().unwrap_or(1))}
                    </h1>
                </PaneHeader>
                <PaneBody>
                    <p class="t-body">"Das ist die zweite Ebene. Schmal ersetzt sie die Liste; ab 768 px steht sie rechts daneben."</p>
                </PaneBody>
            </Pane>
        </SplitView>
    }
}

fn read_hash() -> Option<u32> {
    let hash = window().location().hash().unwrap_or_default();
    hash.strip_prefix(DEMO)?
        .trim_start_matches('/')
        .parse()
        .ok()
}

/// True if the address asks for the demo page instead of the playground.
pub fn is_demo() -> bool {
    window()
        .location()
        .hash()
        .unwrap_or_default()
        .starts_with(DEMO)
}
