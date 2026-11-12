use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "navigation",
    title: "Navigation & Footer",
    group: "Navigation",
    description: "Schwebende Navbar, iOS-Navigation mit großem Titel, Tab-Leiste und Seitenfuß.",
    api: ".ui-navbar · .ui-iosnav · .ui-tabbar · .ui-footer",
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
    let tab = RwSignal::new("home");
    let links = vec![
        NavLink {
            label: "Übersicht",
            href: "#navigation",
            current: true,
        },
        NavLink {
            label: "Produkte",
            href: "#navigation",
            current: false,
        },
        NavLink {
            label: "Lösungen",
            href: "#navigation",
            current: false,
        },
        NavLink {
            label: "Support",
            href: "#navigation",
            current: false,
        },
    ];
    let columns = vec![
        FooterColumn {
            heading: "Entdecken",
            links: vec!["Produkte", "Neuheiten", "Vergleichen"],
        },
        FooterColumn {
            heading: "Unternehmen",
            links: vec!["Über uns", "Karriere", "Presse"],
        },
        FooterColumn {
            heading: "Hilfe",
            links: vec!["Support", "Versand", "Rückgabe"],
        },
        FooterColumn {
            heading: "Kontakt",
            links: vec!["[E-MAIL]", "[TELEFON]", "Standorte"],
        },
    ];

    view! {
        <Section title="Navbar" api="<Navbar brand links> – Kinder erscheinen rechts">
            <div class="doc-backdrop pg-backdrop pg-backdrop--nav">
                <Navbar brand="[MARKE]" links=links>
                    <Button variant=ButtonStyle::Plain shape=ButtonShape::Circle aria_label="Suchen">
                        <Icon path=icons::SEARCH stroke=2.2 />
                    </Button>
                    <Button variant=ButtonStyle::GlassProminent href="#navigation">
                        "Kontakt"
                    </Button>
                </Navbar>
            </div>
        </Section>

        <Section title="iOS-Navigation · TabBar" api="<IosNav title> · <TabBar items selected>">
            <HStack spacing="32px" css="align-items: flex-start">
                <div class="pg-narrow pg-mini-phone">
                    <IosNav title=Signal::derive(move || {
                        TABS.iter().find(|t| t.id == tab.get()).map_or("", |t| t.label).to_string()
                    })>
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
                            <Button variant=ButtonStyle::Plain shape=ButtonShape::Circle aria_label="Neu">
                                <Icon path=icons::PLUS size=19 stroke=2.2 />
                            </Button>
                        </Glass>
                    </IosNav>
                    <div style="margin-top: 24px">
                        <TabBar items=TABS selected=tab />
                    </div>
                </div>
            </HStack>
        </Section>

        <Section title="Footer" api="<Footer columns copyright legal>">
            <Card>
                <Footer
                    columns=columns
                    copyright="© [JAHR] [MARKE]. Alle Rechte vorbehalten."
                    legal=vec!["Impressum", "Datenschutz", "Cookies"]
                />
            </Card>
        </Section>
    }
    .into_any()
}
