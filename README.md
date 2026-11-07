# 🦀 Rust Playground

Ein kleiner, Storybook-ähnlicher Playground für Web-Features, geschrieben in
**Rust** und kompiliert zu **WebAssembly**. Jedes Feature lebt in seinem eigenen
Crate und bekommt im Playground einen eigenen Reiter.

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

## Veröffentlichung (GitHub Pages)

Bei jedem Push auf `main` baut `.github/workflows/pages.yml` die Seite und veröffentlicht sie unter
`https://<user>.github.io/<repo>/`. Einmalig nötig: im Repo unter
**Settings → Pages → Build and deployment → Source** den Wert **GitHub Actions** wählen.

Lokal testen wie auf Pages: `trunk build --release --public-url /rust-playground/`.

## Projektstruktur

```
.
├── Cargo.toml              # Workspace: gemeinsame Versionen & Mitglieder
├── Trunk.toml              # Trunk-Konfiguration (Einstieg: app/index.html)
├── app/                    # Die Playground-App ("Storybook-Hülle")
│   ├── index.html          # Bindet WASM + CSS ein
│   ├── style.css           # Layout von Sidebar & Bühne
│   └── src/
│       ├── main.rs         # Navigation + Rendering der aktiven Story
│       └── stories/        # Eine Datei pro Story + Registry in mod.rs
└── features/               # Wiederverwendbare Features – je ein Crate
    ├── counter/
    ├── temperature/
    ├── todo/
    └── ui/                 # Button, Link, Image (mit Varianten per Props)
```

### Die Idee dahinter

| Begriff     | Wo?                         | Weiß vom Playground? |
|-------------|-----------------------------|----------------------|
| **Feature** | `features/<name>/` (Crate)  | ❌ nein – voll wiederverwendbar |
| **Story**   | `app/src/stories/<name>.rs` | ✅ ja – zeigt das Feature mit Beispiel-Props |

Features hängen nur von `leptos` ab. Du kannst sie in jedem anderen
Leptos-Projekt nutzen, z. B. per Pfad oder Git:

```toml
[dependencies]
feature-counter = { git = "https://github.com/asakrejz/rust-playground" }
```

Gute Praxis: Logik, die keinen Browser braucht (Umrechnungen, Datenmodelle),
als normale Rust-Funktionen schreiben und mit `#[test]` absichern – siehe
`features/temperature` und `features/todo/src/model.rs`.

## Ein neues Feature hinzufügen

Beispiel: ein Feature `clock`.

1. **Crate anlegen**
   ```bash
   cargo new --lib features/clock --name feature-clock
   ```
   In `features/clock/Cargo.toml` die Abhängigkeit eintragen:
   ```toml
   [dependencies]
   leptos = { workspace = true }
   ```
   (Durch `members = ["app", "features/*"]` ist es automatisch Teil des Workspaces.)

2. **Komponente schreiben** in `features/clock/src/lib.rs`:
   ```rust
   use leptos::prelude::*;

   #[component]
   pub fn Clock() -> impl IntoView {
       view! { <p>"Tick"</p> }
   }
   ```
   Optional: `features/clock/style.css` für eigene Styles.

3. **Im Playground registrieren**
   - Root-`Cargo.toml` → `[workspace.dependencies]`:
     `feature-clock = { path = "features/clock" }`
   - `app/Cargo.toml` → `feature-clock = { workspace = true }`
   - `app/src/stories/clock.rs` anlegen (Vorlage: `counter.rs`)
   - `app/src/stories/mod.rs`: `mod clock;` und `clock::STORY` in `STORIES` eintragen
   - Falls CSS vorhanden, in `app/index.html`:
     `<link data-trunk rel="css" href="../features/clock/style.css" />`

Fertig – `trunk serve` zeigt den neuen Reiter, erreichbar unter `#clock`.

## UI-Bausteine (`feature-ui`)

`Button`, `Link` und `Image` werden über typisierte Props gesteuert – Enums statt
Strings, Tippfehler fängt der Compiler ab:

```rust
view! {
    <Button variant=ButtonVariant::Danger size=Size::Lg loading=is_saving>"Löschen"</Button>
    <Link href="https://rust-lang.org" external=true underline=LinkUnderline::Always>"Rust"</Link>
    <Image src="/avatar.png" alt="Avatar" width=96 ratio="1" radius=ImageRadius::Full />
}
```

Zustände wie `variant`, `size`, `disabled` und `loading` des Buttons akzeptieren auch
Signals und lassen sich daher zur Laufzeit ändern (siehe "Live-Controls" in der Button-Story).
Mehrere Stories dürfen auf dasselbe Feature-Crate zeigen.

## Lernpfad durch die Beispiele

1. **Counter** – Komponenten, Props mit Defaults, `signal()`
2. **Temperatur** – Eingabefelder, Events, reine Logik + Unit-Tests
3. **Todo-Liste** – eigenes Datenmodell, `RwSignal`, Formulare, Listen mit `<For>`
4. **Button / Link / Image** – Enums als Props, `Signal`-Props, `Callback`, `Show`, Fehlerbehandlung bei Bildern
