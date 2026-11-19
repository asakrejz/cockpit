use feature_ui::*;
use leptos::prelude::*;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "markdown",
    title: "GitHub-Text",
    group: "Beispiele",
    description: "Beschreibungen und Kommentare aus GitHub (Markdown) als echte Elemente: Überschriften, Listen, Aufgabenlisten, Code, Tabellen, Zitate und Links. Rohes HTML wird nie ausgeführt (Text oder weggelassen), Links nur mit http, https oder mailto, Bilder nur als Link.",
    api: "<Markdown text class> · .ui-rich · .ui-md__task · .ui-rich__code",
    render,
};

const SAMPLE: &str = r#"# Fokus-Ring ergänzen

Dieser PR ergänzt den **Fokus-Ring** für alle *Steuerelemente* – siehe [Issue](https://example.com/issues/1), `focus-visible` und ~~alte Regel~~.
Zeilenumbrüche bleiben wie auf GitHub sichtbar.

## Checkliste

- [x] Tokens ergänzt
- [ ] Doku aktualisieren
- [ ] Screenshot im PR
- normaler Punkt

1. Erster Schritt
2. Zweiter Schritt
   - Unterpunkt

```rust
fn main() {
    println!("Hallo");
}
```

> Ein Zitat aus der Diskussion.

| Name   | Wert | Rechts |
|:-------|:----:|-------:|
| Abstand | 8 px | 1 |
| Radius  | 12 px | 22 |

---

Unsicherer Link: [klick](javascript:alert(1)) · Bild: ![Screenshot](https://example.com/shot.png)

<!-- Kommentar aus der PR-Vorlage, wird weggelassen -->
<script>alert(1)</script>
"#;

fn render() -> AnyView {
    view! {
        <Section title="PR-Beschreibung" api="<Markdown text>">
            <Card>
                <CardBody>
                    <Markdown text=SAMPLE />
                </CardBody>
            </Card>
        </Section>

        <Section title="Kommentar" api="<Markdown text class=\"t-subheadline\">">
            <Card>
                <CardBody>
                    <Markdown text="Bitte **vor dem Merge** prüfen:\n\n- [ ] Tests\n- [x] Lint" class="t-subheadline" />
                </CardBody>
            </Card>
        </Section>
    }
    .into_any()
}
