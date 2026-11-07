use feature_ui::*;
use leptos::prelude::*;

use super::{
    Story,
    common::{Section, Token, find_by_id},
};

pub const STORY: Story = Story {
    id: "button",
    title: "Button",
    group: "Controls",
    description: "Sechs Stile, fünf Größen, Rolle und Form – alles über Props gesteuert. Unten kannst du sie live ändern.",
    api: ".ui-button · data-style · data-control-size · data-role · data-shape",
    render,
};

const STYLES: &[(&str, &str)] = &[
    ("glass-prominent", "glassProminent"),
    ("glass", "glass"),
    ("bordered-prominent", "borderedProminent"),
    ("bordered", "bordered"),
    ("borderless", "borderless"),
    ("plain", "plain"),
];
const SIZES: &[(&str, &str)] = &[
    ("mini", "mini"),
    ("small", "small"),
    ("regular", "regular"),
    ("large", "large"),
    ("extra-large", "XL"),
];
const SHAPES: &[(&str, &str)] = &[
    ("capsule", "Kapsel"),
    ("rounded", "Rounded"),
    ("circle", "Kreis"),
];

fn render() -> AnyView {
    // Zustand der Live-Controls.
    let style_id = RwSignal::new("bordered-prominent");
    let size_id = RwSignal::new("regular");
    let shape_id = RwSignal::new("capsule");
    let destructive = RwSignal::new(false);
    let disabled = RwSignal::new(false);
    let clicks = RwSignal::new(0);

    let variant = Signal::derive(move || {
        find_by_id(&ButtonStyle::ALL, style_id.get(), ButtonStyle::id).unwrap_or_default()
    });
    let size = Signal::derive(move || {
        find_by_id(&ControlSize::ALL, size_id.get(), ControlSize::id).unwrap_or_default()
    });
    let shape = Signal::derive(move || {
        find_by_id(&ButtonShape::ALL, shape_id.get(), ButtonShape::id).unwrap_or_default()
    });

    view! {
        <Section title="Button" api="variant=ButtonStyle::… · size=ControlSize::…">
            <Card>
                <CardBody css="gap: 0; padding: 8px 24px">
                    {ButtonStyle::ALL
                        .map(|v| {
                            view! {
                                <div class="pg-row">
                                    <span class="pg-row__label">
                                        <Token>{v.swift_name()}</Token>
                                    </span>
                                    <Button variant=v size=ControlSize::Small>"Small"</Button>
                                    <Button variant=v>"Regular"</Button>
                                    <Button variant=v size=ControlSize::Large>"Large"</Button>
                                    <Button variant=v size=ControlSize::Large disabled=true>
                                        "Deaktiviert"
                                    </Button>
                                </div>
                            }
                        })
                        .into_iter()
                        .collect_view()}
                    <div class="pg-row">
                        <span class="pg-row__label">
                            <Token>"role · shape"</Token>
                        </span>
                        <Button variant=ButtonStyle::BorderedProminent destructive=true>
                            "Löschen"
                        </Button>
                        <Button destructive=true>"Entfernen"</Button>
                        <Button shape=ButtonShape::Rounded>"Rounded"</Button>
                        <Button shape=ButtonShape::Circle aria_label="Hinzufügen">
                            <Icon path=icons::PLUS stroke=2.2 />
                        </Button>
                        <Button
                            variant=ButtonStyle::BorderedProminent
                            shape=ButtonShape::Circle
                            size=ControlSize::Large
                            aria_label="Weiter"
                        >
                            <Icon path=icons::ARROW_RIGHT size=22 stroke=2.4 />
                        </Button>
                    </div>
                </CardBody>
            </Card>
        </Section>

        <Section title="Liquid Glass" api="data-style=glass · glass-prominent">
            <div class="doc-backdrop pg-backdrop">
                <Badge variant=BadgeVariant::Neutral>"Glas braucht Inhalt dahinter"</Badge>
                <HStack spacing="16px">
                    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>
                        "Weiter"
                    </Button>
                    <Button variant=ButtonStyle::Glass size=ControlSize::Large>"Abbrechen"</Button>
                    <Button
                        variant=ButtonStyle::Glass
                        shape=ButtonShape::Circle
                        size=ControlSize::Large
                        aria_label="Teilen"
                    >
                        <Icon path=icons::SHARE size=20 />
                    </Button>
                    <Button
                        variant=ButtonStyle::Glass
                        shape=ButtonShape::Circle
                        size=ControlSize::Large
                        aria_label="Schließen"
                    >
                        <Icon path=icons::CLOSE stroke=2.4 />
                    </Button>
                    <Button variant=ButtonStyle::Glass destructive=true size=ControlSize::Large>
                        "Löschen"
                    </Button>
                </HStack>
            </div>
        </Section>

        <Section title="Link-Button" api="href=\"…\" → <a class=\"ui-button\">">
            <HStack spacing="12px">
                <Button href="#button" variant=ButtonStyle::GlassProminent size=ControlSize::Large>
                    "Jetzt entdecken"
                </Button>
                <Button href="#button" size=ControlSize::Large>"Mehr erfahren"</Button>
            </HStack>
        </Section>

        <Section title="Live-Controls" api="Signals als Props: variant · size · shape · destructive · disabled">
            <div class="pg-controls">
                <label>
                    <Token>"variant"</Token>
                    <Segmented options=STYLES value=style_id aria_label="Button-Stil" />
                </label>
                <label>
                    <Token>"size"</Token>
                    <Segmented options=SIZES value=size_id aria_label="Größe" />
                </label>
                <label>
                    <Token>"shape"</Token>
                    <Segmented options=SHAPES value=shape_id aria_label="Form" />
                </label>
                <label class="pg-controls__toggle">
                    <Token>"destructive"</Token>
                    <Toggle checked=destructive />
                </label>
                <label class="pg-controls__toggle">
                    <Token>"disabled"</Token>
                    <Toggle checked=disabled />
                </label>
            </div>
            <div class="pg-preview">
                <Button
                    variant=variant
                    size=size
                    shape=shape
                    destructive=destructive
                    disabled=disabled
                    aria_label="Beispiel"
                    on_click=move |_| clicks.update(|c| *c += 1)
                >
                    {move || {
                        if shape.get() == ButtonShape::Circle {
                            view! { <Icon path=icons::PLUS stroke=2.2 /> }.into_any()
                        } else {
                            "Klick mich".into_any()
                        }
                    }}
                </Button>
                <Text variant=TextStyle::Footnote color=TextColor::Secondary>
                    {move || format!("Klicks: {}", clicks.get())}
                </Text>
            </div>
        </Section>
    }
    .into_any()
}
