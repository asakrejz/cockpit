use leptos::prelude::*;

/// Optionally append additional inline styles.
fn style_of(parts: &[Option<String>]) -> Option<String> {
    let joined: String = parts.iter().flatten().map(|p| format!("{p};")).collect();
    (!joined.is_empty()).then_some(joined)
}

/// Vertikaler Stapel (`VStack`).
#[component]
pub fn VStack(
    /// Abstand, z. B. `"12px"` (Standard: `--sys-space-stack`).
    #[prop(optional, into)]
    spacing: Option<String>,
    /// Additional CSS, e.g. `"align-items:center"`.
    #[prop(optional, into)]
    css: Option<String>,
    children: Children,
) -> impl IntoView {
    let style = style_of(&[spacing.map(|s| format!("--spacing:{s}")), css]);
    view! { <div class="ui-vstack" style=style>{children()}</div> }
}

/// Horizontal stack (`HStack`), wraps when there is not enough room.
#[component]
pub fn HStack(
    #[prop(optional, into)] spacing: Option<String>,
    #[prop(optional, into)] css: Option<String>,
    children: Children,
) -> impl IntoView {
    let style = style_of(&[spacing.map(|s| format!("--spacing:{s}")), css]);
    view! { <div class="ui-hstack" style=style>{children()}</div> }
}

/// Layers on top of each other (`ZStack`) – children share one grid cell.
#[component]
pub fn ZStack(#[prop(optional, into)] css: Option<String>, children: Children) -> impl IntoView {
    view! { <div class="ui-zstack" style=css>{children()}</div> }
}

/// Fills free space in an `HStack`.
#[component]
pub fn Spacer() -> impl IntoView {
    view! { <div class="ui-spacer"></div> }
}

/// Grid with `cols` columns (falls to 2 below 1024 px, to 1 below 640 px).
#[component]
pub fn Grid(
    #[prop(default = 3)] cols: u8,
    #[prop(optional, into)] spacing: Option<String>,
    children: Children,
) -> impl IntoView {
    let style = style_of(&[
        Some(format!("--cols:{cols}")),
        spacing.map(|s| format!("--spacing:{s}")),
    ]);
    view! { <div class="ui-grid" style=style>{children()}</div> }
}

/// Two columns, e.g. text + image. Stacks on tablet and phone.
#[component]
pub fn Split(children: Children) -> impl IntoView {
    view! { <div class="ui-split">{children()}</div> }
}

/// Centered content area with a maximum width.
#[component]
pub fn Container(
    /// Schmale, gut lesbare Breite.
    #[prop(optional)]
    readable: bool,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="ui-container" data-width=readable.then_some("readable")>{children()}</div>
    }
}

/// Glass surface (Liquid Glass). Needs content behind it to take effect.
#[component]
pub fn Glass(#[prop(optional, into)] css: Option<String>, children: Children) -> impl IntoView {
    view! { <div class="ui-glass" style=css>{children()}</div> }
}
