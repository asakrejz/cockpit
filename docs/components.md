# Component library (`feature-ui`)

The UI building blocks of Cockpit and the Playground that shows each one.

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
| Layout | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container` |
| Controls | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Input | `TextField`, `SearchField`, `Combobox` (text field with suggestions, see below) |
| Content | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `CardFooter`, `CardGrid`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay | `Sheet` |
| App | `SplitView` (with `StackLevel`: list and detail as two levels below 768 px, see [architecture.md](architecture.md#layout-and-navigation)), `BottomNav` (floating glass pill), `PickerPane`, `Pane`, `Status`, `Avatar`, `Chip`, `InboxRow`, `CommentBubble`, `RichText` (Jira text from ADF), `Markdown` (GitHub text: PR descriptions and comments, same look as `RichText`), `Textarea`, `Diff`, `SuggestionCard`, `Timeline`, `Progress`, `Log`, `Steps` |

**Combobox.** For choosing from a long list (people). The caller reads `query` and passes the ranked `options`
(`ComboOption`, quick choices first); the component brings `role="combobox"`, `aria-expanded`,
`aria-activedescendant`, a listbox, arrow keys, Enter, Escape, mouse and touch. The ticket detail uses it for the
assignee (matching and ranking: `cockpit_core::people_search`).

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

**Own tokens (`extras.css`).** Own additions have the prefix `--ui-*`, so a new `tokens.css` never collides with them.
They have the same value in light and dark.

| Token | Value | Used for |
|-------|-------|----------|
| `--ui-overlay` | `rgb(0 0 0 / 0.35)` | the dimmed area behind a sheet, a dialog and the "Mehr" sheet |
| `--ui-fill-hover` | `rgb(127 127 127 / 0.12)` | hover fill of a row that is a button (section heading of a list) |
| `--ui-shadow-float` | `0 6px 24px rgb(0 0 0 / 0.12)` | floating notices (banners) |
| `--ui-shadow-sheet` | `0 -8px 40px rgb(0 0 0 / 0.18)` | dialog that rises from the bottom edge |
| `--ui-dialog-width` | `560px` | width of a dialog on wide screens |
| `--ui-pill-*`, `--ui-nav-space` | see [architecture.md](architecture.md#layout-and-navigation) | navigation pill and the space below the panes |
| `--ui-pane-pad`, `--ui-card-gap` | `--ref-space-20` (`--ref-space-16` below 1100 px), `--ref-space-12` | inner padding of a pane, gap between cards |

Fixed sizes of Cockpit have one name each at the top of `apps/cockpit/style.css`: `--ck-icon-size` (round reason icon,
checkbox on touch screens), `--ck-notice-width` (banners), `--ck-toast-width`, `--ck-column-basis`, `--ck-setup-gap`,
`--ck-bullet-indent`. A new fixed size gets a name there. Raw pixel values remain only for media-query breakpoints,
border and outline widths, and the tracks and limits of a layout (`minmax`, `max-height`).

**Utility classes (`extras.css`, last block).** Markup has no `style` attribute for a static value; it uses these
classes (each sets one property, from the tokens). `VStack`, `HStack` and `CardBody` take them in `class`
(`<HStack class="ui-gap-8 ui-justify-between">`).

| Class | Sets |
|-------|------|
| `ui-gap-0` … `ui-gap-20` (0, 2, 4, 6, 8, 10, 12, 16, 20) | `--spacing`: gap of `ui-vstack`, `ui-hstack`, `ui-grid` (`--ref-space-N`) |
| `ui-grow` / `ui-flex-none` | `flex: 1 1 auto; min-width: 0` / `flex: none` |
| `ui-flex-wrap` / `ui-flex-nowrap` | `flex-wrap` |
| `ui-justify-between` / `-end` / `-center` | `justify-content` |
| `ui-items-start` | `align-items: flex-start` |
| `ui-self-start` | `align-self: flex-start` |
| `ui-max-full` | `max-width: 100%` |
| `ui-px-4`, `ui-py-8`, `ui-mt-8` | padding inline 4, padding block 8, margin top 8 (`--ref-space-N`) |
| `ui-text-nowrap` / `ui-text-pre-line` / `ui-text-center` / `ui-text-break` | `white-space: nowrap`, `white-space: pre-line`, `text-align: center`, `overflow-wrap: anywhere` |
| `ui-icon-tint` / `ui-icon-muted` | an icon in its own span, in the accent colour / the secondary label colour |
| `ui-segmented--fill`, `ui-card__body--loose` | a `Segmented` whose options share the full width; a card body with 12 px gap |
| `ck-problem` | the red text colour of an error message (`--sys-color-red`) |

A `style` attribute stays only where the value is computed at run time: the width of a bar, the indent of a tree row,
the colour of a row from its tone, `--p`, `--slots` and `--active` of the sliders and the navigation pill. It then
uses a token or a custom property, never a raw colour.

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

**Overflow and cards.** Nothing may widen a pane or the page. In `extras.css`: prose breaks with
`overflow-wrap: anywhere`, flex/grid children have `min-width: 0`, single-line labels (repo, branch, file) use
`.ui-ellipsis` / `.ck-ellipsis` with a `title`, code blocks scroll inside their box. A card is a column: `CardBody`
grows, `CardFooter` (name, meta, actions) sits at the bottom right whatever the text length. `CardGrid` puts cards in
one row at equal height, so footers line up. Overview cards (`.ck-stuck`) and inbox rows follow the same pattern
(last child pinned to the bottom).

**Check.** The Playground story "Extreme Inhalte" (`#extreme`) has very long words and URLs, one-word and empty texts.
Open it at 375, 768 and 1280 px and run `scripts/check-overflow.js` in the console: it must print `no overflow`
(`scrollWidth <= clientWidth` for page, panes, cards and rows). Run the same script in the app.
