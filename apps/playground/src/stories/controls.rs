use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "controls",
    title: "Toggle · Picker · Select · Slider · Stepper",
    group: "Controls",
    description: "Die Eingabe-Controls sind an Signals gebunden und im Playground direkt bedienbar.",
    api: ".ui-toggle · .ui-segmented · .ui-select · .ui-slider · .ui-stepper",
    render,
};

const VIEWS: &[(&str, &str)] = &[("raster", "Raster"), ("liste", "Liste"), ("karte", "Karte")];

fn render() -> AnyView {
    let wifi = RwSignal::new(true);
    let notify = RwSignal::new(false);
    let view_mode = RwSignal::new("liste");
    let level = RwSignal::new(60);
    let count = RwSignal::new(1);
    let project = RwSignal::new(String::new());
    let projects = Signal::derive(|| {
        [
            ("", "Alle Projekte"),
            ("BAU", "BAU · Baukasten"),
            ("WEB", "WEB · Webseite"),
        ]
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .to_vec()
    });

    view! {
        <Section title="Controls in einer Liste" api="<Toggle checked=RwSignal<bool>> · <Segmented> · <Select> · <Slider> · <Stepper>">
            <div class="pg-narrow">
                <List>
                    <ListSection footer="Toggle 64 × 28 mit Kapsel-Thumb (iOS 26). Beim Drücken wird der Thumb breiter.">
                        <ListRow title="WLAN" toggle_row=true>
                            <Toggle checked=wifi aria_label="WLAN" />
                        </ListRow>
                        <ListRow
                            title="Mitteilungen"
                            subtitle="Tint: --comp-toggle-on → green"
                            toggle_row=true
                        >
                            <Toggle checked=notify aria_label="Mitteilungen" />
                        </ListRow>
                        <ListRow title="Ansicht">
                            <Segmented options=VIEWS value=view_mode aria_label="Ansicht" />
                        </ListRow>
                        <ListRow title="Projekt">
                            <Select
                                options=projects
                                value=project
                                on_change=move |v: String| project.set(v)
                                aria_label="Projekt"
                                compact=true
                            />
                        </ListRow>
                        <ListRow title="Helligkeit">
                            <div class="pg-slider-row">
                                <Icon path=icons::DOT size=16 />
                                <Slider value=level aria_label="Helligkeit" />
                                <Icon path=icons::SUN size=22 />
                            </div>
                        </ListRow>
                        <ListRow title="Anzahl">
                            <Text variant=TextStyle::Body color=TextColor::Secondary>
                                {move || count.get().to_string()}
                            </Text>
                            <Stepper value=count />
                        </ListRow>
                    </ListSection>
                </List>
                <p class="t-footnote t-secondary pg-state">
                    {move || {
                        format!(
                            "WLAN: {} · Mitteilungen: {} · Ansicht: {} · Projekt: {} · Helligkeit: {}%",
                            if wifi.get() { "an" } else { "aus" },
                            if notify.get() { "an" } else { "aus" },
                            view_mode.get(),
                            project.with(|p| if p.is_empty() { "alle".to_string() } else { p.clone() }),
                            level.get(),
                        )
                    }}
                </p>
            </div>
        </Section>
    }
    .into_any()
}
