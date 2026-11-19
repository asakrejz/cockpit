use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "combobox",
    title: "Combobox",
    group: "Controls",
    description: "Textfeld mit Vorschlagsliste für lange Listen (Personen): Pfeiltasten, Enter, Escape, Maus und Touch.",
    api: "<Combobox label value query options on_select placeholder empty_text busy> · ComboOption",
    render,
};

const NAMES: [(&str, &str); 6] = [
    ("Mira Keller", "MK"),
    ("Timo Böhm", "TB"),
    ("Lena Ammer", "LA"),
    ("Amelie Kraus", "AK"),
    ("Jörg Sommer", "JS"),
    ("Anna Maier", "AM"),
];

/// Case and accents do not matter: „bohm“ finds „Böhm“.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .replace(['ä', 'á', 'à'], "a")
        .replace(['ö', 'ó', 'ò'], "o")
        .replace(['ü', 'ú', 'ù'], "u")
        .replace(['é', 'è'], "e")
}

fn render() -> AnyView {
    let query = RwSignal::new(String::new());
    let chosen = RwSignal::new("Niemand".to_string());
    let options = Signal::derive(move || {
        let needle = fold(query.get().trim());
        let mut list = vec![ComboOption::new("Ich", "Ich").initials("ICH").divider()];
        if needle.chars().count() >= 2 {
            list.extend(
                NAMES
                    .iter()
                    .filter(|(name, _)| fold(name).contains(&needle))
                    .map(|(name, initials)| ComboOption::new(*name, *name).initials(*initials)),
            );
        }
        list
    });
    view! {
        <Section title="Combobox" api="<Combobox> · role=combobox · aria-activedescendant">
            <div class="pg-narrow">
                <Card>
                    <CardBody css="gap: 20px">
                        <Combobox
                            label="Zugewiesen an"
                            value=chosen
                            query=query
                            options=options
                            on_select=move |id: String| chosen.set(id)
                            placeholder="Name eingeben"
                            empty_text="Niemand gefunden"
                        />
                    </CardBody>
                </Card>
                <p class="t-footnote t-secondary pg-state">
                    {move || format!("Gewählt: {}", chosen.get())}
                </p>
            </div>
        </Section>
    }
    .into_any()
}
