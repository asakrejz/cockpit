# 🦀 Rust Playground

Ein Storybook-ähnlicher Playground für Web-Bausteine, geschrieben in **Rust** und
kompiliert zu **WebAssembly**. Jeder Baustein bekommt im Playground einen eigenen Reiter.
Das Aussehen kommt aus dem Design **„Baukasten"** (iOS 26 / Liquid Glass) und wird
über Design-Tokens gesteuert.

- UI-Framework: [Leptos](https://leptos.dev) (Client-Side Rendering)
- Build-Tool: [Trunk](https://trunkrs.dev) (Rust → WASM → fertige Website)

## Schnellstart

```bash
# einmalig
rustup target add wasm32-unknown-unknown   # passiert dank rust-toolchain.toml auch automatisch
cargo install trunk --locked

# Entwicklungsserver mit Live-Reload auf http://localhost:8080
trunk serve --open

# Tests (laufen nativ, ohne Browser)
cargo test

# Produktions-Build nach ./dist
trunk build --release
```

## Projektstruktur

```
.
├── Cargo.toml                  # Workspace: gemeinsame Versionen & Mitglieder
├── Trunk.toml                  # Trunk-Konfiguration (Einstieg: app/index.html)
├── styles/
│   └── tokens.css              # Design-Tokens – für das GANZE Projekt (siehe unten)
├── features/
│   └── ui/                     # Wiederverwendbare Bausteine (Crate `feature-ui`)
│       ├── components.css      # Komponenten-CSS, 1:1 aus dem Design
│       ├── extras.css          # kleine Rust-spezifische Ergänzungen
│       └── src/                # Leptos-Komponenten, eine Datei pro Baustein-Gruppe
└── app/                        # Der Playground ("Storybook-Hülle")
    ├── index.html              # bindet WASM + CSS ein
    ├── style.css               # Layout der Hülle – nur Tokens, keine festen Farben
    └── src/
        ├── main.rs             # Navigation, Theme-Schalter, Rendering der aktiven Story
        └── stories/            # eine Datei pro Story + Registry in mod.rs
```

### Die Idee dahinter

| Begriff     | Wo?                         | Weiß vom Playground? |
|-------------|-----------------------------|----------------------|
| **Baustein** | `features/<name>/` (Crate) | ❌ nein – voll wiederverwendbar |
| **Story**   | `app/src/stories/<name>.rs` | ✅ ja – zeigt den Baustein mit Beispiel-Props |

Ein Feature-Crate hängt nur von `leptos` ab. In einem anderen Leptos-Projekt:

```toml
[dependencies]
feature-ui = { git = "https://github.com/asakrejz/rust-playground" }
```

…und `styles/tokens.css`, `features/ui/components.css`, `features/ui/extras.css` einbinden
(in dieser Reihenfolge).

## Design-Tokens

`styles/tokens.css` hat drei Ebenen:

| Ebene | Präfix | Inhalt | Beispiel |
|-------|--------|--------|----------|
| Primitive | `--ref-*` | Rohwerte | `--ref-space-16`, `--ref-radius-12` |
| Semantisch | `--sys-*` | SwiftUI-Rollen, theme-aufgelöst | `--sys-color-label`, `--sys-type-headline` |
| Komponente | `--comp-*` | pro Baustein | `--comp-button-height-large` |

Eigenes CSS soll **nur `--sys-*` (und bei Bedarf `--ref-*`)** benutzen – dann funktionieren
Hell/Dunkel, Akzentfarbe und „Kontrast erhöhen" automatisch. Siehe `app/style.css`.

Modi werden über Attribute gesetzt, am besten über die `Root`-Komponente:

```rust
view! { <Root theme=Theme::Dark tint=Tint::Pink contrast=Contrast::High>{/* … */}</Root> }
```

Die Tokens werden am `data-theme`-Element neu aufgelöst. Der Playground zeigt alle
Werte in der Story **Foundations** und hat links unten Schalter für Theme, Akzentfarbe
und Kontrast.

> **Design aktualisieren:** `styles/tokens.css` und `features/ui/components.css` sind
> unverändert aus dem Design übernommen. Bei Änderungen im Design einfach die Dateien
> ersetzen. Ergänzungen gehören in `extras.css`.

## Bausteine (`feature-ui`)

Die API spiegelt die SwiftUI-Modifier des Designs. Statt Strings gibt es Enums –
Tippfehler fängt der Compiler ab.

```rust
view! {
    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Weiter"</Button>
    <Button shape=ButtonShape::Circle aria_label="Hinzufügen"><Icon path=icons::PLUS /></Button>
    <Toggle checked=wifi />
    <Segmented options=VIEWS value=view_mode aria_label="Ansicht" />
    <TextField label="E-Mail" value=mail input_type="email" error=invalid />
}
```

| Gruppe | Komponenten |
|--------|-------------|
| Grundlagen | `Root`, `Text`, `Icon` (+ `icons::*`), `Glass` |
| Layout | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container`, `PageSection` |
| Controls | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Eingabe | `TextField`, `SearchField` |
| Inhalt | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay | `Sheet` |

Controls sind an Signals gebunden (`RwSignal`). `variant`, `size`, `shape`, `destructive`
und `disabled` des Buttons akzeptieren feste Werte **oder** Signals – siehe „Live-Controls"
in der Button-Story.

Die Enums werden gegen `components.css` getestet (`cargo test`): Passt eine ID nicht mehr zu
einem CSS-Selektor, schlägt der Test fehl.

## Eine neue Story hinzufügen

1. Datei `app/src/stories/mein_baustein.rs` anlegen (Vorlage: `link_badge.rs`):
   ```rust
   use feature_ui::*;
   use leptos::prelude::*;
   use super::{Story, common::Section};

   pub const STORY: Story = Story {
       id: "mein-baustein",            // erscheint in der URL: #mein-baustein
       title: "Mein Baustein",
       group: "Controls",              // Überschrift in der Navigation
       description: "Was zeigt die Story?",
       api: ".ui-… · data-…",          // Kurzreferenz
       render,
   };

   fn render() -> AnyView {
       view! { <Section title="Beispiel">{/* … */}</Section> }.into_any()
   }
   ```
2. In `app/src/stories/mod.rs`: `mod mein_baustein;` und `mein_baustein::STORY` in `STORIES` eintragen.

## Einen neuen Baustein hinzufügen

Passt er zu `feature-ui`, ergänze eine Komponente dort (Klasse + `data-*`-Attribute aus dem
Design). Soll er ein eigenes Crate werden:

1. `cargo new --lib features/mein-baustein --name feature-mein-baustein`
2. In dessen `Cargo.toml`: `leptos = { workspace = true }` (durch `members = ["app", "features/*"]` ist es automatisch im Workspace)
3. Root-`Cargo.toml` → `[workspace.dependencies]`: `feature-mein-baustein = { path = "features/mein-baustein" }`, dann in `app/Cargo.toml` eintragen.
4. Eigenes CSS in `app/index.html` per `<link data-trunk rel="css" …>` einbinden, **nach** den Tokens.

## Veröffentlichung (GitHub Pages)

Bei jedem Push auf `main` baut `.github/workflows/pages.yml` die Seite und veröffentlicht sie unter
`https://<user>.github.io/<repo>/`. Einmalig nötig: im Repo unter
**Settings → Pages → Build and deployment → Source** den Wert **GitHub Actions** wählen.

Lokal testen wie auf Pages: `trunk build --release --public-url /rust-playground/`.

## Schrift

Das Design nutzt SF Pro (auf Apple-Geräten vorhanden) und fällt sonst auf **Inter** zurück.
`app/index.html` lädt Inter von Google Fonts. Wer keine externe Verbindung möchte, entfernt
die drei `<link>`-Zeilen – dann greift der System-Font.
