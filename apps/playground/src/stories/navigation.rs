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

        <Section
            title="Untere Navigation · BottomNav"
            api="<BottomNav label more_label more_title more_current more_badge slots active solid more> · <BottomNavItem href label icon current badge>"
        >
            <p class="t-footnote t-secondary">
                "Schwebende Glas-Pille auf jeder Breite: nur so breit wie ihre Einträge, mittig, ohne Balken und ohne Hintergrundfläche. "
                "Vier feste Ziele und „Mehr“, das ein Sheet mit den übrigen öffnet; eine Kapsel gleitet zum aktuellen Eintrag "
                "(nicht bei „Bewegung reduzieren“). Echte iOS-Lichtbrechung gibt es im Web nicht: Weichzeichner, Sättigung, helle Kante und Glanz nähern sie an."
            </p>
            <div class="pg-bottomnav-grid">
                <div>
                    <p class="t-footnote t-secondary">"Hell"</p>
                    <div class="pg-bottomnav-frame" data-theme="light">{pill_demo(false)}</div>
                </div>
                <div>
                    <p class="t-footnote t-secondary">"Dunkel"</p>
                    <div class="pg-bottomnav-frame" data-theme="dark">{pill_demo(false)}</div>
                </div>
                <div>
                    <p class="t-footnote t-secondary">"Fallback (deckend: reduzierte Transparenz oder kein backdrop-filter)"</p>
                    <div class="pg-bottomnav-frame" data-theme="light">{pill_demo(true)}</div>
                </div>
            </div>
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

/// The pill over a colourful backdrop, so the glass has something to blur.
fn pill_demo(solid: bool) -> impl IntoView {
    view! {
        <div class="pg-bottomnav-backdrop" aria-hidden="true"></div>
        <BottomNav
            label="Hauptnavigation"
            more_label="Mehr"
            more_title="Mehr"
            more_badge=2usize
            solid=solid
            slots=5
            active=Some(0)
            more=move || {
                view! {
                    <ul class="ui-sidebar__nav">
                        <SidebarItem href="#navigation" label="Team" icon=icons::USERS count="2".to_string() />
                        <SidebarItem href="#navigation" label="Protokoll" icon=icons::LIBRARY />
                        <SidebarItem href="#navigation" label="Einrichten" icon=icons::SLIDERS />
                    </ul>
                }
            }
        >
            <BottomNavItem href="#navigation" label="Inbox" icon=icons::BELL current=true badge=7usize />
            <BottomNavItem href="#navigation" label="Aufgaben" icon=icons::TASKS />
            <BottomNavItem href="#navigation" label="Reviews" icon=icons::GIT_PR badge=3usize />
            <BottomNavItem href="#navigation" label="Agenten" icon=icons::SPARKLES />
        </BottomNav>
    }
}
