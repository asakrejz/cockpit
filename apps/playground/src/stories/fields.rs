use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "fields",
    title: "TextField & Search",
    group: "Controls",
    description: "Textfelder mit Label, Hinweis und Fehlerzustand sowie das runde Suchfeld.",
    api: ".ui-field · .ui-search · data-state=\"error\"",
    render,
};

fn render() -> AnyView {
    let query = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let mail = RwSignal::new(String::new());
    let zip = RwSignal::new("1234".to_string());
    // The postal code is invalid as long as it does not consist of exactly five digits.
    let zip_invalid = Signal::derive(move || {
        let v = zip.get();
        v.len() != 5 || !v.chars().all(|c| c.is_ascii_digit())
    });

    view! {
        <Section title="TextField · Search" api="<TextField label value placeholder hint error> · <SearchField value placeholder>">
            <div class="pg-narrow">
                <Card>
                    <CardBody css="gap: 20px">
                        <SearchField value=query />
                        <TextField label="Name" value=name placeholder="Vor- und Nachname" />
                        <TextField
                            label="E-Mail"
                            value=mail
                            input_type="email"
                            placeholder="name@beispiel.de"
                            hint="Wir geben deine Adresse nicht weiter."
                        />
                        <TextField
                            label="Postleitzahl"
                            value=zip
                            hint="Bitte eine fünfstellige PLZ eingeben."
                            error=zip_invalid
                        />
                    </CardBody>
                </Card>
                <p class="t-footnote t-secondary pg-state">
                    {move || {
                        format!(
                            "Suche: „{}“ · PLZ {}",
                            query.get(),
                            if zip_invalid.get() { "ungültig" } else { "gültig" },
                        )
                    }}
                </p>
            </div>
        </Section>
    }
    .into_any()
}
