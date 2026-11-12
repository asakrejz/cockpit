use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "link-badge",
    title: "Link & Badge",
    group: "Controls",
    description: "Links im Fließtext oder mit Chevron, dazu Badges in drei Varianten.",
    api: ".ui-link[data-variant=chevron] · .ui-badge[data-variant=neutral|count]",
    render,
};

fn render() -> AnyView {
    view! {
        <Section title="Link" api="<Link href chevron>">
            <Card>
                <CardBody css="gap: 16px">
                    <p class="t-body">
                        "Fließtext mit einem " <Link href="#link-badge">"eingebetteten Link"</Link>
                        "."
                    </p>
                    <span class="t-title3">
                        <Link href="#link-badge" chevron=true>"Mehr erfahren"</Link>
                    </span>
                </CardBody>
            </Card>
        </Section>

        <Section title="Badge" api="variant=BadgeVariant::Tint | Neutral | Count">
            <HStack>
                <Badge>"Neu"</Badge>
                <Badge variant=BadgeVariant::Neutral>"Bald verfügbar"</Badge>
                <Badge variant=BadgeVariant::Count>"3"</Badge>
            </HStack>
        </Section>
    }
    .into_any()
}
