use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "pager",
    title: "Pager",
    group: "Navigation",
    description: "Karussell mit Glas-Pfeilen und Seitenpunkten. Der Track gleitet mit --sys-motion-smooth.",
    api: ".ui-pager · .ui-pager__track · .ui-pager__dots",
    render,
};

fn page(n: usize, eyebrow: &'static str, title: &'static str) -> AnyView {
    view! {
        <Card>
            <Split>
                <Media ratio=MediaRatio::R4x3 placeholder=format!("[BILD {n}]") />
                <CardBody css="padding: 56px; gap: 10px">
                    <Text variant=TextStyle::Subheadline weight=TextWeight::Semibold color=TextColor::Tint>
                        {eyebrow}
                    </Text>
                    <Text variant=TextStyle::LargeTitle weight=TextWeight::Bold tag=TextTag::Span>
                        {title}
                    </Text>
                    <Text variant=TextStyle::Body color=TextColor::Secondary>
                        "[Ein bis zwei Sätze zum Highlight.]"
                    </Text>
                    <div style="margin-top: 14px">
                        <Button variant=ButtonStyle::BorderedProminent href="#pager">"Ansehen"</Button>
                    </div>
                </CardBody>
            </Split>
        </Card>
    }
    .into_any()
}

fn render() -> AnyView {
    let pages = vec![
        page(1, "Neu", "[Highlight eins.]"),
        page(2, "Design", "[Highlight zwei.]"),
        page(3, "Leistung", "[Highlight drei.]"),
        page(4, "Service", "[Highlight vier.]"),
    ];
    view! {
        <Section title="TabView(.page)" api="<Pager pages=Vec<AnyView> label>">
            <Pager pages=pages label="Highlights" />
        </Section>
    }
    .into_any()
}
