use feature_ui::*;
use leptos::prelude::*;

use super::{
    Story,
    common::{Section, Token},
};

pub const STORY: Story = Story {
    id: "layout",
    title: "Stacks & Grids",
    group: "Navigation",
    description: "Layout-Bausteine, benannt wie in SwiftUI. Abstand über spacing, Spalten über cols. Grids fallen unter 1024 px auf 2, unter 640 px auf 1 Spalte.",
    api: ".ui-vstack · .ui-hstack · .ui-zstack · .ui-grid · .ui-split",
    render,
};

fn block(css: &'static str) -> impl IntoView {
    view! { <div class="pg-block" style=css></div> }
}

fn render() -> AnyView {
    view! {
        <Section title="Stacks" api="<VStack> · <HStack> + <Spacer> · <ZStack>">
            <Grid cols=3>
                <Card>
                    <CardBody css="gap: 14px">
                        <Token>"VStack"</Token>
                        <VStack spacing="8px">
                            {block("height: 36px")} {block("height: 36px")} {block("height: 36px")}
                        </VStack>
                    </CardBody>
                </Card>
                <Card>
                    <CardBody css="gap: 14px">
                        <Token>"HStack + Spacer"</Token>
                        <HStack spacing="8px" css="flex-wrap: nowrap">
                            {block("width: 56px; height: 124px")}
                            <Spacer />
                            {block("width: 56px; height: 124px")}
                            {block("width: 56px; height: 124px")}
                        </HStack>
                    </CardBody>
                </Card>
                <Card>
                    <CardBody css="gap: 14px">
                        <Token>"ZStack"</Token>
                        <ZStack css="height: 124px">
                            {block("border-radius: 20px")}
                            <div
                                class="t-footnote t-semibold"
                                style="align-self: end; justify-self: end; margin: 10px; padding: 8px 14px; border-radius: 9999px; background: var(--sys-color-tint); color: var(--sys-color-on-tint)"
                            >
                                "Overlay"
                            </div>
                        </ZStack>
                    </CardBody>
                </Card>
            </Grid>
        </Section>

        <Section title="Grid" api="<Grid cols=4>">
            <Grid cols=4>
                {["1", "2", "3", "4"]
                    .map(|n| {
                        view! {
                            <div class="pg-block pg-block--cell t-headline">{n}</div>
                        }
                    })
                    .into_iter()
                    .collect_view()}
            </Grid>
        </Section>

        <Section title="Split" api="<Split> · Text + Bild, stapelt auf Tablet und Phone">
            <Card>
                <CardBody css="padding: 48px">
                    <Split>
                        <VStack spacing="10px">
                            <Text variant=TextStyle::LargeTitle weight=TextWeight::Bold tag=TextTag::H3>
                                "[Feature-Überschrift.]"
                            </Text>
                            <Text variant=TextStyle::Body color=TextColor::Secondary>
                                "[Zwei bis drei Sätze, die das Feature erklären. Die Spalten stapeln sich auf Tablet und Phone.]"
                            </Text>
                            <div style="margin-top: 8px">
                                <Button href="#layout">"Details"</Button>
                            </div>
                        </VStack>
                        <Media ratio=MediaRatio::R4x3 placeholder="[BILD 4:3]" />
                    </Split>
                </CardBody>
            </Card>
        </Section>
    }
    .into_any()
}
