# Component library (`feature-ui`)

The UI building blocks of Cockpit (crate `feature-ui` in `crates/ui`) and the Playground (`apps/playground`) that shows each
one. The crate mirrors the SwiftUI names of the design:

```rust
view! {
    <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Next"</Button>
    <Toggle checked=wifi />
    <TextField label="Email" value=mail input_type="email" error=invalid />
}
```

| Group | Components |
|-------|------------|
| Basics | `Root`, `Text`, `Icon`, `Glass` |
| Layout | `VStack`, `HStack`, `ZStack`, `Spacer`, `Grid`, `Split`, `Container` |
| Controls | `Button`, `Link`, `Badge`, `Toggle`, `Segmented`, `Slider`, `Stepper` |
| Input | `TextField`, `SearchField`, `Select`, `Combobox` (text field with suggestions, see below) |
| Content | `List`, `ListSection`, `ListRow`, `ListButtonRow`, `Card`, `CardBody`, `CardFooter`, `CardGrid`, `Media` |
| Navigation | `Navbar`, `IosNav`, `TabBar`, `Pager`, `Footer` |
| Overlay | `Sheet`, `Dialog` |
| State | `EmptyState`, `ErrorState`, `StatusBanner`, `Skeleton` |
| App | `SplitView` (with `StackLevel`: list and detail as two levels below 768 px, see [architecture.md](architecture.md#layout-and-navigation)), `BottomNav` (floating glass pill), `PickerPane`, `Pane`, `Status`, `Avatar`, `Chip`, `InboxRow`, `CommentBubble`, `RichText` (Jira text from ADF), `Markdown` (GitHub text: PR descriptions and comments, same look as `RichText`), `Textarea`, `Diff`, `SuggestionCard`, `Timeline`, `Progress`, `Log`, `Steps` |

**Combobox.** For choosing from a long list (people). The caller reads `query` and passes the ranked `options`
(`ComboOption`, quick choices first); the component brings `role="combobox"`, `aria-expanded`,
`aria-activedescendant`, a listbox, arrow keys, Enter, Escape, mouse and touch. The ticket detail uses it for the
assignee (matching and ranking: `cockpit_core::people_search`).

**Where things live.** The building blocks are modules of `crates/ui/src` (declared in `lib.rs`); the crate depends on
`leptos`, `cockpit-core` and `cockpit-i18n`. A story lives in `apps/playground/src/stories/<name>.rs` and shows a block with
sample values. In another project include the crate by path to your checkout:

```toml
[dependencies]
feature-ui = { path = "<path to the checkout>/crates/ui" }
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
| `--ui-overlay` | `rgb(0 0 0 / 0.35)` | the dimmed area behind a sheet, a dialog and the "More" sheet |
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

**Add a building block.**

1. Create `crates/ui/src/my_block.rs` with a `#[component]` (template: any small module such as `status.rs`), declare it in
   `crates/ui/src/lib.rs` (`mod my_block;` and `pub use my_block::*;`).
2. Style it in `crates/ui/extras.css` with the tokens above (own classes with the prefix `ui-`); never in `tokens.css` or
   `components.css`.
3. Show it with a story (see above). A visible text goes through the catalogs (`t!`), never as a literal in `crates/ui/src`
   (a scan test fails otherwise, see [architecture.md](architecture.md#languages-i18n)).

**Font.** The design uses SF Pro (present on Apple devices) and falls back to **Inter**. `apps/playground/index.html`
loads Inter from Google Fonts. Without an external connection remove the three `<link>` lines and the system font
applies.

**Overflow and cards.** Nothing may widen a pane or the page. In `extras.css`: prose breaks with
`overflow-wrap: anywhere`, flex/grid children have `min-width: 0`, single-line labels (repo, branch, file) use
`.ui-ellipsis` / `.ck-ellipsis` with a `title`, code blocks scroll inside their box. A card is a column: `CardBody`
grows, `CardFooter` (name, meta, actions) sits at the bottom right whatever the text length. `CardGrid` puts cards in
one row at equal height, so footers line up. Overview cards (`.ck-stuck`) and inbox rows follow the same pattern
(last child pinned to the bottom).

**Check.** The Playground story with the id `extreme` (`#extreme`) has very long words and URLs, one-word and empty texts.
Open it at 375, 768 and 1280 px and run `scripts/check-overflow.js` in the console: it must print `no overflow`
(`scrollWidth <= clientWidth` for page, panes, cards and rows). Run the same script in the app.
