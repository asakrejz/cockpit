use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "foundations",
    title: "Foundations",
    group: "Grundlagen",
    description: "Die Design-Tokens: Farben, Text Styles, Radien, Abstände und Schatten. Alles liegt in crates/ui/tokens.css und gilt für das ganze Projekt.",
    api: "--ref-* (Rohwerte) · --sys-* (Rollen) · --comp-* (pro Baustein)",
    render,
};

const COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "mint", "teal", "cyan", "blue", "indigo", "purple", "pink",
    "brown",
];

const SURFACES: &[&str] = &[
    "background",
    "secondary-background",
    "grouped-background",
    "label",
    "secondary-label",
    "tertiary-label",
    "fill",
    "secondary-fill",
    "tertiary-fill",
    "quaternary-fill",
    "separator",
    "tint",
    "tint-fill",
];

const RADII: &[&str] = &["control", "field", "icon", "media", "container", "sheet"];
const SPACES: &[u32] = &[4, 8, 12, 16, 20, 32, 48];

fn swatch(name: &'static str) -> impl IntoView {
    view! {
        <div class="pg-swatch-card">
            <div
                class="pg-swatch-card__chip"
                style=format!("background: var(--sys-color-{name})")
            ></div>
            <span class="doc-token">{format!("--sys-color-{name}")}</span>
        </div>
    }
}

fn render() -> AnyView {
    view! {
        <Section title="System-Farben" api="--sys-color-<name> · passt sich Theme und Kontrast an">
            <div class="pg-swatch-grid">{COLORS.iter().map(|c| swatch(c)).collect_view()}</div>
        </Section>

        <Section title="Flächen, Label & Füllungen" api="--sys-color-label · -fill · -background …">
            <div class="pg-swatch-grid">{SURFACES.iter().map(|c| swatch(c)).collect_view()}</div>
        </Section>

        <Section title="Text Styles" api=".t-<name> · --sys-type-<name>">
            <Card>
                <CardBody css="padding: 8px 24px; gap: 0">
                    {TextStyle::ALL
                        .map(|style| {
                            view! {
                                <div class="pg-type-row">
                                    <span class="doc-token">{format!(".t-{}", style.id())}</span>
                                    <Text variant=style>"Baukasten Aa"</Text>
                                </div>
                            }
                        })
                        .into_iter()
                        .collect_view()}
                </CardBody>
            </Card>
        </Section>

        <Section title="Radien" api="--sys-radius-<name>">
            <HStack spacing="20px">
                {RADII
                    .iter()
                    .map(|name| {
                        view! {
                            <VStack spacing="6px">
                                <div
                                    class="pg-radius-box"
                                    style=format!("border-radius: var(--sys-radius-{name})")
                                ></div>
                                <span class="doc-token">{format!("--sys-radius-{name}")}</span>
                            </VStack>
                        }
                    })
                    .collect_view()}
            </HStack>
        </Section>

        <Section title="Abstände" api="--ref-space-<pt> · Punkte wie in SwiftUI">
            <VStack spacing="8px">
                {SPACES
                    .iter()
                    .map(|n| {
                        view! {
                            <div class="pg-space-row">
                                <span class="doc-token">{format!("--ref-space-{n}")}</span>
                                <div
                                    class="pg-space-bar"
                                    style=format!("width: var(--ref-space-{n})")
                                ></div>
                            </div>
                        }
                    })
                    .collect_view()}
            </VStack>
        </Section>

        <Section title="Schatten" api="--ref-shadow-1 · -2 · -3">
            <HStack spacing="24px">
                {[1, 2, 3]
                    .map(|n| {
                        view! {
                            <div
                                class="pg-shadow-box"
                                style=format!("box-shadow: var(--ref-shadow-{n})")
                            >
                                <span class="doc-token">{format!("--ref-shadow-{n}")}</span>
                            </div>
                        }
                    })
                    .into_iter()
                    .collect_view()}
            </HStack>
        </Section>
    }
    .into_any()
}
