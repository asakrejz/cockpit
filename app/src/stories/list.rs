use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "list",
    title: "List",
    group: "Inhalt",
    description: "Gruppierte Listen im iOS-Stil mit Icons, Details, Kopf- und Fußzeilen.",
    api: ".ui-list · .ui-list__section · .ui-list__group[data-icons] · .ui-list__row",
    render,
};

fn render() -> AnyView {
    let airplane = RwSignal::new(false);

    view! {
        <Section title="List · insetGrouped" api="<List> · <ListSection icons> · <ListRow title icon detail chevron href>">
            <Grid cols=2 spacing="32px">
                <List>
                    <ListSection icons=true>
                        <ListRow title="Flugmodus" icon=(Tint::Orange, icons::AIRPLANE) toggle_row=true>
                            <Toggle checked=airplane aria_label="Flugmodus" />
                        </ListRow>
                        <ListRow
                            title="WLAN"
                            icon=(Tint::Blue, icons::WIFI)
                            detail="[NETZWERK]"
                            chevron=true
                            href="#list"
                        />
                        <ListRow
                            title="Bluetooth"
                            icon=(Tint::Blue, icons::BLUETOOTH)
                            detail="Ein"
                            chevron=true
                            href="#list"
                        />
                        <ListRow
                            title="Mitteilungen"
                            icon=(Tint::Red, icons::BELL)
                            chevron=true
                            href="#list"
                        >
                            <Badge variant=BadgeVariant::Count>"2"</Badge>
                        </ListRow>
                        <ListRow
                            title="Fokus"
                            icon=(Tint::Indigo, icons::MOON)
                            chevron=true
                            href="#list"
                        />
                    </ListSection>
                </List>

                <List>
                    <ListSection
                        header="Konto"
                        footer="Footer-Text erklärt eine Sektion in Footnote-Größe."
                    >
                        <ListRow title="Profil bearbeiten" chevron=true href="#list" />
                        <ListRow title="Passwort & Sicherheit" chevron=true href="#list" />
                    </ListSection>
                    <ListSection>
                        <ListButtonRow title="Abmelden" destructive=true />
                    </ListSection>
                </List>
            </Grid>
        </Section>
    }
    .into_any()
}
