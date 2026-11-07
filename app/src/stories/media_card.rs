use feature_ui::*;
use leptos::prelude::*;

use super::{
    Story,
    common::{Section, Token},
};

pub const STORY: Story = Story {
    id: "media-card",
    title: "Image & Card",
    group: "Inhalt",
    description: "Bilder mit festem Seitenverhältnis (mit Platzhalter und Fehler-Fallback) und Karten in drei Varianten.",
    api: ".ui-media[data-ratio] · .ui-card[data-interactive]",
    render,
};

/// Beispielbild als SVG-Data-URI (kein Netzwerk nötig). `#` muss als `%23` kodiert werden.
const SAMPLE: &str = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='640' height='360' viewBox='0 0 640 360' preserveAspectRatio='none'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='%23ff9500'/><stop offset='1' stop-color='%23af52de'/></linearGradient></defs><rect width='100%25' height='100%25' fill='url(%23g)'/><circle cx='320' cy='180' r='90' fill='white' fill-opacity='0.85'/></svg>";

fn render() -> AnyView {
    view! {
        <Section title="Image" api="<Media ratio=MediaRatio::… shape=MediaShape::Circle src alt>">
            <div class="pg-ratio-grid">
                {MediaRatio::ALL
                    .map(|r| {
                        view! {
                            <VStack spacing="6px">
                                <Media ratio=r />
                                <Token>{format!("data-ratio=\"{}\"", r.id())}</Token>
                            </VStack>
                        }
                    })
                    .into_iter()
                    .collect_view()}
                <VStack spacing="6px">
                    <Media shape=MediaShape::Circle />
                    <Token>"data-shape=\"circle\""</Token>
                </VStack>
            </div>
        </Section>

        <Section title="Echtes Bild & Fehler-Fallback" api="src=\"…\" – lädt das Bild nicht, erscheint der Platzhalter">
            <div class="pg-ratio-grid">
                <VStack spacing="6px">
                    <Media src=SAMPLE alt="Farbverlauf in Orange und Violett" />
                    <Token>"src=\"data:image/svg+xml…\""</Token>
                </VStack>
                <VStack spacing="6px">
                    <Media
                        src="/gibt-es-nicht.png"
                        alt="Dieses Bild existiert nicht"
                        placeholder="[Bild nicht verfügbar]"
                    />
                    <Token>"src=\"/gibt-es-nicht.png\""</Token>
                </VStack>
            </div>
        </Section>

        <Section title="Card" api="<Card href interactive elevated> · <CardBody>">
            <Grid cols=3>
                <Card href="#media-card">
                    <Media ratio=MediaRatio::R4x3 src=SAMPLE alt="Produktbild" />
                    <CardBody>
                        <span style="align-self: flex-start">
                            <Badge>"Neu"</Badge>
                        </span>
                        <Text variant=TextStyle::Title2 weight=TextWeight::Bold tag=TextTag::Span>
                            "[PRODUKTNAME]"
                        </Text>
                        <Text variant=TextStyle::Subheadline color=TextColor::Secondary>
                            "[Kurzbeschreibung in ein bis zwei Zeilen.]"
                        </Text>
                    </CardBody>
                </Card>
                <Card href="#media-card">
                    <Media ratio=MediaRatio::R4x3 />
                    <CardBody>
                        <Text
                            variant=TextStyle::Footnote
                            weight=TextWeight::Semibold
                            color=TextColor::Secondary
                        >
                            "[KATEGORIE]"
                        </Text>
                        <Text variant=TextStyle::Title2 weight=TextWeight::Bold tag=TextTag::Span>
                            "[TITEL]"
                        </Text>
                        <Text variant=TextStyle::Subheadline color=TextColor::Secondary>
                            "[Kurzbeschreibung in ein bis zwei Zeilen.]"
                        </Text>
                    </CardBody>
                </Card>
                <Card elevated=true>
                    <CardBody css="justify-content: space-between; height: 100%; gap: 24px">
                        <VStack spacing="6px">
                            <Text
                                variant=TextStyle::Footnote
                                weight=TextWeight::Semibold
                                color=TextColor::Secondary
                            >
                                "GroupBox · ohne Bild"
                            </Text>
                            <Text variant=TextStyle::Title2 weight=TextWeight::Bold tag=TextTag::Span>
                                "[ÜBERSCHRIFT]"
                            </Text>
                            <Text variant=TextStyle::Subheadline color=TextColor::Secondary>
                                "[Text für Feature-Kacheln oder Kennzahlen.]"
                            </Text>
                        </VStack>
                        <HStack>
                            <Button variant=ButtonStyle::BorderedProminent>"Aktion"</Button>
                            <Button variant=ButtonStyle::Borderless>"Später"</Button>
                        </HStack>
                    </CardBody>
                </Card>
            </Grid>
        </Section>
    }
    .into_any()
}
