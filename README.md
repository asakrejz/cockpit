# Rust Playground

Ein Lernprojekt: **Web-Bausteine in Rust, kompiliert zu WebAssembly.** Jeder Baustein
bekommt im Playground einen eigenen Reiter, ähnlich wie bei Storybook. So lernst du Rust
an kleinen, sichtbaren Beispielen und baust nebenbei eine Bibliothek, die du in anderen
Projekten wiederverwenden kannst.

Im Repo liegen zwei Apps, die sich die Bausteine teilen:

- **Playground** (`app/`): zeigt jeden Baustein einzeln.
- **Cockpit** (`cockpit/`): ein Dashboard für Jira, GitHub und Agenten, gebaut aus denselben Bausteinen.

Technik:

- UI-Framework: [Leptos](https://leptos.dev) (läuft komplett im Browser)
- Build-Tool: [Trunk](https://trunkrs.dev) (Rust → WASM → fertige Seite)
- Aussehen: Design **„Baukasten"** (iOS 26 / Liquid Glass), gesteuert über Design-Tokens

## Schnellstart

```bash
# einmalig
rustup target add wasm32-unknown-unknown   # passiert dank rust-toolchain.toml auch automatisch
cargo install trunk --locked

trunk serve --open                  # Playground auf http://localhost:8080
(cd cockpit && trunk serve --open)  # Cockpit auf http://localhost:8081
cargo test                          # Tests laufen nativ, ohne Browser
trunk build --release               # fertige Seite nach ./dist
```

Beim Lernen hilft: `trunk serve` laufen lassen, eine Datei ändern, Speichern – der Browser
lädt neu, und der Compiler sagt dir sofort, was nicht passt.

## Backend starten

Das Backend (`server/`, Binary `cockpit-server`) ist ein lokaler Dienst für eine Person. Es lauscht nur
auf `127.0.0.1`, liefert die gebaute Cockpit-App aus und hält Daten (SQLite) und Schlüssel
(Schlüsselbund des Systems) – Schlüssel verlassen das Backend nie.

```bash
(cd cockpit && trunk build)     # App nach cockpit/dist bauen
cargo run -p cockpit-server     # http://127.0.0.1:8787
curl localhost:8787/api/health  # {"status":"ok","version":"…","verbindungen":{"github":false,"jira":false}}
```

Konfiguration (alles optional): Datei `config.toml` im Konfigurationsverzeichnis (`~/.config/cockpit/`
unter Linux), darüber Umgebungsvariablen `COCKPIT_PORT` (8787), `COCKPIT_DIST` (`cockpit/dist`),
`COCKPIT_DB` (Datenverzeichnis des Nutzers, `cockpit/cockpit.db`) und `COCKPIT_CONFIG` (Pfad der Datei).

```toml
port = 8787
[github]                          # api_url ist optional
[jira]
site = "https://firma.atlassian.net"
email = "ich@firma.example"
```

Schlüssel stehen nie in der Datei. Sie kommen über die Standardeingabe in den Schlüsselbund:
`echo "$TOKEN" | cargo run -p cockpit-server -- token setzen github` (oder `jira`; `token loeschen <name>` entfernt sie).
`/api/health` meldet je Verbindung nur ja/nein. Die Server-Crates (`server/`, `crates/integrations`)
laufen nur nativ; für Wasm schließt die CI sie aus (siehe `docs/ROADMAP.md`).

## Lernpfad

Die Konzepte bauen aufeinander auf. Jeder Schritt nennt die Stelle im Code, an der du es
siehst, und die Story im Playground, in der es sichtbar wird.

| # | Konzept | Im Code | Im Playground |
|---|---------|---------|---------------|
| 1 | **Komponenten und Props**: eine Funktion mit `#[component]`, Parameter sind Props | `features/ui/src/text.rs` (`Text`) | Foundations |
| 2 | **Enums statt Strings**: `enum`, `impl`, `#[derive(Default)]`, `#[default]`, `const ALL` | `features/ui/src/button.rs` (`ButtonStyle`) | Button |
| 3 | **`Option` und `then_some`**: ein Attribut nur manchmal setzen | `button.rs`, Funktion `attr()` und `data-role` | Button |
| 4 | **Children**: Inhalt an eine Komponente übergeben | `features/ui/src/layout.rs` (`VStack`) | Stacks & Grids |
| 5 | **Signals**: reaktiver Zustand, `RwSignal`, `.get()` / `.set()` | `features/ui/src/controls.rs` (`Toggle`) | Toggle · Picker · … |
| 6 | **Closures mit `move`**: warum Signals in `move \|\| …` stehen | überall, z. B. `data-role=role` in `button.rs` | Button |
| 7 | **Abgeleitete Signals**: `Signal::derive` | `app/src/stories/button.rs` | Button (Live-Controls) |
| 8 | **Callbacks**: Ereignisse nach oben melden | `button.rs` (`on_click`) | Button |
| 9 | **Listen rendern**: Iteratoren und `collect_view()` | `controls.rs` (`Segmented`) | Toggle · Picker · … |
| 10 | **Bedingt rendern**: `<Show>` und `match` mit `.into_any()` | `sheet.rs`, `list.rs` (`ListRow`) | iPhone-Beispiel |
| 11 | **`'static` und Konstanten**: `&'static [(&str, &str)]`, `const`, `static` | `controls.rs`, `app/src/stories/mod.rs` | alle |
| 12 | **Funktionszeiger**: `render: fn() -> AnyView` in einer Registry | `app/src/stories/mod.rs` (`Story`) | die Navigation selbst |
| 13 | **`Option` und `Result` verketten**: `.ok().flatten().is_some_and(…)` | `app/src/main.rs` (`system_theme`) | Hell/Dunkel-Schalter |
| 14 | **Ereignisse aus dem Browser**: `window_event_listener`, Aufräumen mit `on_cleanup` | `app/src/main.rs` | Navigation per URL (`#button`) |
| 15 | **Globaler Zähler**: `AtomicU32` für eindeutige IDs | `features/ui/src/field.rs` | TextField & Search |
| 16 | **Tests**: `#[cfg(test)]`, `include_str!` liest das CSS zur Compile-Zeit | `features/ui/src/lib.rs` | `cargo test` |
| 17 | **Logik ohne UI**: `struct`, `impl`, `Result`, Zustandsmaschine, viele Tests | `crates/cockpit-core/src/agent.rs`, `workspace.rs` | Cockpit |
| 18 | **Gemeinsamer Zustand**: ein `Copy`-Store im Kontext, `Memo`, `Effect` | `cockpit/src/store.rs`, `cockpit/src/pages/inbox.rs` | Cockpit |
| 19 | **Workspace und Crates**: gemeinsame Versionen, ein Crate pro Baustein | `Cargo.toml`, `features/ui/Cargo.toml` | – |

Gute Übung für jeden Schritt: Ändere etwas absichtlich falsch (z. B. `ButtonStyle::Glas`
statt `Glass`) und lies die Compiler-Meldung. Rust erklärt Fehler meist sehr genau.

## Projektstruktur

```
.
├── Cargo.toml                  # Workspace: gemeinsame Versionen & Mitglieder
├── Trunk.toml                  # Trunk-Konfiguration (Einstieg: app/index.html)
├── styles/
│   └── tokens.css              # Design-Tokens, für das ganze Projekt
├── features/
│   └── ui/                     # wiederverwendbare Bausteine (Crate `feature-ui`)
│       ├── components.css      # Komponenten-CSS, 1:1 aus dem Design
│       ├── extras.css          # Ergänzungen und Korrekturen zum Design
│       └── src/                # Leptos-Komponenten, eine Datei pro Gruppe
├── crates/
│   ├── cockpit-core/           # Fachlogik des Cockpits – ohne UI, mit Tests
│   └── integrations/           # GitHub- und Jira-Clients (nur lesen), nativ
├── server/                     # Backend: axum, SQLite, Schlüsselablage
├── cockpit/                    # die Cockpit-App (eigenes index.html und Trunk.toml)
│   └── src/
│       ├── main.rs             # Start, Routing, Takt der Agenten
│       ├── route.rs            # Seiten und ihre Adressen (#/review/87)
│       ├── store.rs            # gemeinsamer Zustand
│       └── pages/              # Einrichten, Inbox, Review, Agent
└── app/                        # der Playground
    ├── index.html              # bindet WASM und CSS ein
    ├── style.css               # Layout der Hülle (nur Tokens, keine festen Farben)
    └── src/
        ├── main.rs             # Navigation, Theme-Schalter, aktive Story
        └── stories/            # eine Datei pro Story + Registry in mod.rs
```

### Baustein und Story

| Begriff      | Wo?                         | Kennt den Playground? |
|--------------|-----------------------------|-----------------------|
| **Baustein** | `features/<name>/` (Crate)  | nein, daher wiederverwendbar |
| **Story**    | `app/src/stories/<name>.rs` | ja, zeigt den Baustein mit Beispielwerten |

Ein Baustein-Crate hängt nur von `leptos` ab. In einem anderen Projekt bindest du ihn
per Pfad ein:

```toml
[dependencies]
feature-ui = { path = "../rust-playground/features/ui" }
```

Dazu `styles/tokens.css`, `features/ui/components.css` und `features/ui/extras.css`
einbinden, in dieser Reihenfolge.

## Design-Tokens

`styles/tokens.css` hat drei Ebenen:

| Ebene      | Präfix    | Inhalt                          | Beispiel |
|------------|-----------|---------------------------------|----------|
| Primitive  | `--ref-*` | Rohwerte                        | `--ref-space-16` |
| Semantisch | `--sys-*` | Rollen, abhängig vom Theme      | `--sys-color-label` |
| Komponente | `--comp-*`| pro Baustein                    | `--comp-button-height-large` |

Eigenes CSS benutzt nur `--sys-*` (bei Bedarf `--ref-*`). Dann funktionieren Hell/Dunkel,
Akzentfarbe und „Kontrast erhöhen" automatisch. Alle Werte siehst du in der Story
**Foundations**, und links unten im Playground stellst du Theme, Akzentfarbe und Kontrast um.

> **Design aktualisieren:** `tokens.css` und `components.css` sind unverändert aus dem
> Design übernommen. Bei Änderungen ersetzt du einfach die Dateien. Eigene Ergänzungen
> gehören in `extras.css`.

## Bausteine (`feature-ui`)

Die API spiegelt die SwiftUI-Namen des Designs:

```rust
view! {
    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Weiter"</Button>
    <Toggle checked=wifi />
    <TextField label="E-Mail" value=mail input_type="email" error=invalid />
}
```

| Gruppe     | Komponenten |
|------------|-------------|
| Grundlagen | `Root`, `Text`, `Icon`, `Glass` |
| Layout     | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container`, `PageSection` |
| Controls   | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Eingabe    | `TextField`, `SearchField` |
| Inhalt     | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay    | `Sheet` |
| App        | `SplitView`, `Sidebar`, `Pane`, `Status`, `Avatar`, `Chip`, `InboxRow`, `CommentBubble`, `Textarea`, `Diff`, `SuggestionCard`, `Timeline`, `Progress`, `Log`, `Steps` |

## Cockpit

Ein Dashboard, das Jira, GitHub und Agenten an einem Ort zusammenführt. Fünf Seiten:

| Seite | Adresse | Was sie kann |
|-------|---------|--------------|
| Einrichten | `#/einrichten` | Jira, GitHub und Agent verbinden; festlegen, was der Agent darf |
| Aufmerksamkeit | `#/`, `#/meine`, `#/team` | Inbox aus Tickets und Pull Requests; Status ändern, Branch erstellen, antworten, Agent beauftragen; Suche und Quellen-Filter; Tasten `↑↓`/`j k`, `e`, `r`, `/` |
| Review | `#/review/87` | Diff lesen; Zeilen auswählen und eigene Kommentare schreiben; Kommentare des Agenten freigeben, ablehnen oder bearbeiten; Review senden |
| Übersicht | `#/uebersicht` | Pro Person offene Einträge und Auslastung; „Hängt“ zeigt fehlgeschlagene Checks, wartende Reviews, Tickets ohne Branch und Agenten, die auf dich warten |
| Agent | `#/agent/BAU-142` | Auftrag mit Optionen; Plan zur Freigabe (abschaltbar); Ablauf in fünf Schritten bis zum Pull Request |

Zwei Regeln sind in der Logik verankert und getestet: Ein Review wird erst gesendet, wenn über
jeden Vorschlag entschieden ist (eigene Zeilenkommentare brauchen keine Freigabe). Der Agent legt vor dem Start einen Plan vor und tut nichts, bis du ihn freigibst (ablehnen oder mit Anmerkung neu anfordern ist möglich). Und der Agent merged nie – sein Ergebnis ist ein Pull Request,
der auf dein Review wartet.

### Stand: Demo ohne Backend

Die Oberfläche und die Abläufe sind vollständig, die Daten sind Beispieldaten
(`crates/cockpit-core/src/demo.rs`). **Es gibt noch keine echte Verbindung zu Jira, GitHub oder
einem Modell-Anbieter.** Im Einzelnen:

- Einrichten prüft das Format der Eingaben, nicht die Schlüssel selbst.
- Schlüssel werden nie gespeichert. Im Browser (`localStorage`) liegen nur die Haken „verbunden" und die
  Berechtigungen (`cockpit.connections`) sowie der Stand der Beispieldaten (`cockpit.workspace`).
- Der Agent ist ein Timer, der die fünf Schritte durchläuft. Sein „Review" sind einfache Regeln
  (feste Farbwerte, feste Pixelwerte, entfernter Fokus-Ring).
- Der Stand der Beispieldaten (gelesen, Status, Branches, Kommentare, Reviews, Agenten-Läufe) überlebt das
  Neuladen. Ein laufender Agent macht danach an seinem Schritt weiter. Gespeichert wird gebündelt (nicht bei
  jedem Tastendruck) als JSON mit Versionsnummer, siehe `crates/cockpit-core/src/persist.rs`. Passt die Version
  nicht oder sind die Daten unlesbar, startet die App mit den Beispieldaten und überschreibt den alten Stand.
  Neue Felder in den Typen brauchen `#[serde(default)]`, damit ältere Stände lesbar bleiben.
- Unter „Einrichten“ › Übersicht setzt „Demo-Daten zurücksetzen“ (mit Rückfrage) alles auf den Ausgangszustand.
  Die Verbindungen bleiben.

Für den echten Betrieb braucht es ein Backend, das die Schlüssel hält und die Aktionen aus
`cockpit-core::Workspace` gegen die Dienste ausführt. Schlüssel gehören nicht in den Browser,
und ein Agent, der Code ändert und testet, braucht eine Umgebung zum Ausführen.

## Eigene Story hinzufügen

1. Datei `app/src/stories/mein_baustein.rs` anlegen, Vorlage ist `link_badge.rs`:
   ```rust
   use feature_ui::*;
   use leptos::prelude::*;
   use super::{Story, common::Section};

   pub const STORY: Story = Story {
       id: "mein-baustein",       // erscheint in der URL: #mein-baustein
       title: "Mein Baustein",
       group: "Controls",         // Überschrift in der Navigation
       description: "Was zeigt die Story?",
       api: ".ui-… · data-…",     // Kurzreferenz
       render,
   };

   fn render() -> AnyView {
       view! { <Section title="Beispiel">{/* … */}</Section> }.into_any()
   }
   ```
2. In `app/src/stories/mod.rs` `mod mein_baustein;` ergänzen und `mein_baustein::STORY`
   in `STORIES` eintragen.

## Eigenen Baustein als neues Crate

1. `cargo new --lib features/mein-baustein --name feature-mein-baustein`
2. In dessen `Cargo.toml`: `leptos = { workspace = true }`. Durch `members = ["app", "features/*"]`
   gehört es automatisch zum Workspace.
3. Im Root-`Cargo.toml` unter `[workspace.dependencies]` eintragen:
   `feature-mein-baustein = { path = "features/mein-baustein" }`, dann in `app/Cargo.toml` verwenden.
4. Eigenes CSS in `app/index.html` per `<link data-trunk rel="css" …>` einbinden, nach den Tokens.

## Schrift

Das Design nutzt SF Pro (auf Apple-Geräten vorhanden) und fällt sonst auf **Inter** zurück.
`app/index.html` lädt Inter von Google Fonts. Ohne externe Verbindung entfernst du die drei
`<link>`-Zeilen, dann greift der System-Font.
