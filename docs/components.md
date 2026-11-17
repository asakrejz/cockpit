# Component library (`feature-ui`)

The UI building blocks of Cockpit and the Playground that shows each one. Back to the [README](../README.md).

The crate `crates/ui` mirrors the SwiftUI names of the design:

```rust
view! {
    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Weiter"</Button>
    <Toggle checked=wifi />
    <TextField label="E-Mail" value=mail input_type="email" error=invalid />
}
```

| Group | Components |
|-------|------------|
| Basics | `Root`, `Text`, `Icon`, `Glass` |
| Layout | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container`, `PageSection` |
| Controls | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Input | `TextField`, `SearchField` |
| Content | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay | `Sheet` |
| App | `SplitView`, `Sidebar`, `BottomNav`, `Pane`, `Status`, `Avatar`, `Chip`, `InboxRow`, `CommentBubble`, `Textarea`, `Diff`, `SuggestionCard`, `Timeline`, `Progress`, `Log`, `Steps` |

**Building block and story.** A building block lives in `crates/<name>/` (a crate that depends only on `leptos`, hence
reusable); a story lives in `apps/playground/src/stories/<name>.rs` and shows it with sample values. In another
project include it by path:

```toml
[dependencies]
feature-ui = { path = "../rust-playground/crates/ui" }
```

and include `crates/ui/tokens.css`, `components.css` and `extras.css`, in this order.

**Design tokens.** `crates/ui/tokens.css` has three layers:

| Layer | Prefix | Content | Example |
|-------|--------|---------|---------|
| Primitive | `--ref-*` | raw values | `--ref-space-16` |
| Semantic | `--sys-*` | roles, theme dependent | `--sys-color-label` |
| Component | `--comp-*` | per building block | `--comp-button-height-large` |

Your own CSS uses only `--sys-*` (if needed `--ref-*`); then light/dark, accent colour and "increase contrast" work
automatically. All values are visible in the story **Foundations**; theme, accent and contrast are switched at the
bottom left of the Playground. *Updating the design:* `tokens.css` and `components.css` are taken unchanged from the
design; replace the files on changes. Own additions go into `extras.css`.

**Add your own story.**

1. Create `apps/playground/src/stories/my_block.rs` (template: `link_badge.rs`):
   ```rust
   use feature_ui::*;
   use leptos::prelude::*;
   use super::{Story, common::Section};

   pub const STORY: Story = Story {
       id: "my-block",            // appears in the URL: #my-block
       title: "My block",
       group: "Controls",         // heading in the navigation
       description: "What does the story show?",
       api: ".ui-… · data-…",     // short reference
       render,
   };

   fn render() -> AnyView {
       view! { <Section title="Example">{/* … */}</Section> }.into_any()
   }
   ```
2. In `apps/playground/src/stories/mod.rs` add `mod my_block;` and register `my_block::STORY` in `STORIES`.

**Add a building block as a new crate.**

1. `cargo new --lib crates/my-block --name feature-my-block`
2. In its `Cargo.toml`: `leptos = { workspace = true }`. Through `members = [..., "crates/*"]` it is part of the
   workspace automatically.
3. Register it in the root `Cargo.toml` under `[workspace.dependencies]`:
   `feature-my-block = { path = "crates/my-block" }`, then use it in `apps/playground/Cargo.toml`.
4. Include your own CSS in `apps/playground/index.html` with `<link data-trunk rel="css" …>`, after the tokens.

**Font.** The design uses SF Pro (present on Apple devices) and falls back to **Inter**. `apps/playground/index.html`
loads Inter from Google Fonts. Without an external connection remove the three `<link>` lines and the system font
applies.

## Learning path

For learning Rust through this code base: each concept names where you see it and the Playground story that shows it.

| # | Concept | In the code | In the Playground |
|---|---------|-------------|-------------------|
| 1 | **Components and props**: a function with `#[component]`, parameters are props | `crates/ui/src/text.rs` (`Text`) | Foundations |
| 2 | **Enums instead of strings**: `enum`, `impl`, `#[derive(Default)]`, `#[default]`, `const ALL` | `crates/ui/src/button.rs` (`ButtonStyle`) | Button |
| 3 | **`Option` and `then_some`**: set an attribute only sometimes | `button.rs`, function `attr()` and `data-role` | Button |
| 4 | **Children**: pass content to a component | `crates/ui/src/layout.rs` (`VStack`) | Stacks & Grids |
| 5 | **Signals**: reactive state, `RwSignal`, `.get()` / `.set()` | `crates/ui/src/controls.rs` (`Toggle`) | Toggle · Picker · … |
| 6 | **`move` closures**: why signals sit in `move \|\| …` | everywhere, e.g. `data-role=role` in `button.rs` | Button |
| 7 | **Derived signals**: `Signal::derive` | `apps/playground/src/stories/button.rs` | Button (live controls) |
| 8 | **Callbacks**: report events upwards | `button.rs` (`on_click`) | Button |
| 9 | **Rendering lists**: iterators and `collect_view()` | `controls.rs` (`Segmented`) | Toggle · Picker · … |
| 10 | **Conditional rendering**: `<Show>` and `match` with `.into_any()` | `sheet.rs`, `list.rs` (`ListRow`) | iPhone example |
| 11 | **`'static` and constants**: `&'static [(&str, &str)]`, `const`, `static` | `controls.rs`, `apps/playground/src/stories/mod.rs` | all |
| 12 | **Function pointers**: `render: fn() -> AnyView` in a registry | `apps/playground/src/stories/mod.rs` (`Story`) | the navigation itself |
| 13 | **Chaining `Option` and `Result`**: `.ok().flatten().is_some_and(…)` | `apps/playground/src/main.rs` (`system_theme`) | light/dark switch |
| 14 | **Browser events**: `window_event_listener`, cleanup with `on_cleanup` | `apps/playground/src/main.rs` | navigation by URL (`#button`) |
| 15 | **Global counter**: `AtomicU32` for unique IDs | `crates/ui/src/field.rs` | TextField & Search |
| 16 | **Tests**: `#[cfg(test)]`, `include_str!` reads the CSS at compile time | `crates/ui/src/lib.rs` | `cargo test` |
| 17 | **Logic without UI**: `struct`, `impl`, `Result`, state machine, many tests | `crates/cockpit-core/src/agent.rs`, `workspace.rs` | Cockpit |
| 18 | **Shared state**: a `Copy` store in context, `Memo`, `Effect` | `apps/cockpit/src/store.rs`, `apps/cockpit/src/pages/inbox.rs` | Cockpit |
| 19 | **Workspace and crates**: shared versions, one crate per building block | `Cargo.toml`, `crates/ui/Cargo.toml` | – |

A good exercise for every step: break something on purpose (for example `ButtonStyle::Glas` instead of `Glass`) and
read the compiler message. Rust usually explains errors very precisely.
