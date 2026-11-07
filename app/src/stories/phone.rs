use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "phone",
    title: "iPhone-Beispiel",
    group: "Beispiele",
    description: "Alle Bausteine zusammen in einem Screen: Navigation, Suche, Liste, Tab-Leiste und ein Sheet.",
    api: "<IosNav> · <SearchField> · <List> · <TabBar> · <Sheet>",
    render,
};

const TABS: &[TabItem] = &[
    TabItem {
        id: "home",
        label: "Start",
        icon: icons::HOME,
    },
    TabItem {
        id: "library",
        label: "Mediathek",
        icon: icons::LIBRARY,
    },
    TabItem {
        id: "settings",
        label: "Einstellungen",
        icon: icons::SETTINGS,
    },
];

fn render() -> AnyView {
    let tab = RwSignal::new("settings");
    let airplane = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let sheet_open = RwSignal::new(false);
    let entry = RwSignal::new(String::new());

    let title = Signal::derive(move || {
        TABS.iter()
            .find(|t| t.id == tab.get())
            .map_or("", |t| t.label)
            .to_string()
    });

    view! {
        <Section title="Beispiel-Screen" api="390 × 844 · Tippe auf „+“, um das Sheet zu öffnen">
            <div class="pg-phone">
                <div class="pg-phone__scroll">
                    <IosNav title=title>
                        <Button
                            variant=ButtonStyle::Glass
                            shape=ButtonShape::Circle
                            size=ControlSize::Large
                            aria_label="Zurück"
                        >
                            <Icon path=icons::CHEVRON_LEFT size=20 stroke=2.4 />
                        </Button>
                        <Glass css="display: inline-flex; gap: 2px; padding: 0; border-radius: 9999px">
                            <Button variant=ButtonStyle::Plain shape=ButtonShape::Circle aria_label="Teilen">
                                <Icon path=icons::SHARE size=19 />
                            </Button>
                            <Button
                                variant=ButtonStyle::Plain
                                shape=ButtonShape::Circle
                                aria_label="Neu"
                                on_click=move |_| sheet_open.set(true)
                            >
                                <Icon path=icons::PLUS size=19 stroke=2.2 />
                            </Button>
                        </Glass>
                    </IosNav>

                    <div style="margin-bottom: 20px">
                        <SearchField value=query />
                    </div>

                    <List>
                        <ListSection>
                            <ListRow title="[Name]" subtitle="Konto, Abos & mehr" chevron=true href="#phone">
                            </ListRow>
                        </ListSection>
                        <ListSection icons=true>
                            <ListRow title="Flugmodus" icon=(Tint::Orange, icons::AIRPLANE) toggle_row=true>
                                <Toggle checked=airplane aria_label="Flugmodus" />
                            </ListRow>
                            <ListRow
                                title="WLAN"
                                icon=(Tint::Blue, icons::WIFI)
                                detail=Signal::derive(move || {
                                    if airplane.get() { "Aus" } else { "[NETZWERK]" }.to_string()
                                })
                                chevron=true
                                href="#phone"
                            />
                            <ListRow
                                title="Bluetooth"
                                icon=(Tint::Blue, icons::BLUETOOTH)
                                detail="Ein"
                                chevron=true
                                href="#phone"
                            />
                        </ListSection>
                        <ListSection icons=true>
                            <ListRow
                                title="Mitteilungen"
                                icon=(Tint::Red, icons::BELL)
                                chevron=true
                                href="#phone"
                            >
                                <Badge variant=BadgeVariant::Count>"2"</Badge>
                            </ListRow>
                            <ListRow
                                title="Fokus"
                                icon=(Tint::Indigo, icons::MOON)
                                chevron=true
                                href="#phone"
                            />
                        </ListSection>
                    </List>
                </div>

                <nav aria-label="Tabs" class="pg-phone__tabs">
                    <div style="flex: 1 1 auto">
                        <TabBar items=TABS selected=tab />
                    </div>
                    <Button
                        variant=ButtonStyle::Glass
                        shape=ButtonShape::Circle
                        aria_label="Suchen"
                        size=ControlSize::Large
                    >
                        <Icon path=icons::SEARCH size=22 stroke=2.2 />
                    </Button>
                </nav>

                <Sheet open=sheet_open title="Neuer Eintrag">
                    <TextField label="Titel" value=entry placeholder="Titel eingeben" />
                </Sheet>
            </div>
        </Section>
    }
    .into_any()
}
