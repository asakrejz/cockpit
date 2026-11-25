use leptos::{ev, prelude::*};

use crate::{Icon, icons};

/// `.buttonStyle(…)` – `data-style`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    GlassProminent,
    Glass,
    BorderedProminent,
    #[default]
    Bordered,
    Borderless,
    Plain,
}

impl ButtonStyle {
    pub const ALL: [ButtonStyle; 6] = [
        ButtonStyle::GlassProminent,
        ButtonStyle::Glass,
        ButtonStyle::BorderedProminent,
        ButtonStyle::Bordered,
        ButtonStyle::Borderless,
        ButtonStyle::Plain,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ButtonStyle::GlassProminent => "glass-prominent",
            ButtonStyle::Glass => "glass",
            ButtonStyle::BorderedProminent => "bordered-prominent",
            ButtonStyle::Bordered => "bordered",
            ButtonStyle::Borderless => "borderless",
            ButtonStyle::Plain => "plain",
        }
    }

    /// Spelling as in SwiftUI.
    pub fn swift_name(self) -> &'static str {
        match self {
            ButtonStyle::GlassProminent => ".glassProminent",
            ButtonStyle::Glass => ".glass",
            ButtonStyle::BorderedProminent => ".borderedProminent",
            ButtonStyle::Bordered => ".bordered",
            ButtonStyle::Borderless => ".borderless",
            ButtonStyle::Plain => ".plain",
        }
    }
}

/// `.controlSize(…)` – `data-control-size`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlSize {
    Mini,
    Small,
    #[default]
    Regular,
    Large,
    ExtraLarge,
}

impl ControlSize {
    pub const ALL: [ControlSize; 5] = [
        ControlSize::Mini,
        ControlSize::Small,
        ControlSize::Regular,
        ControlSize::Large,
        ControlSize::ExtraLarge,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ControlSize::Mini => "mini",
            ControlSize::Small => "small",
            ControlSize::Regular => "regular",
            ControlSize::Large => "large",
            ControlSize::ExtraLarge => "extra-large",
        }
    }

    /// `None` = default size, omit the attribute.
    pub fn attr(self) -> Option<&'static str> {
        (self != ControlSize::Regular).then(|| self.id())
    }
}

/// Shape of the button – `data-shape`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonShape {
    /// Kapsel (Standard).
    #[default]
    Capsule,
    Rounded,
    Circle,
}

impl ButtonShape {
    pub const ALL: [ButtonShape; 3] = [
        ButtonShape::Capsule,
        ButtonShape::Rounded,
        ButtonShape::Circle,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ButtonShape::Capsule => "capsule",
            ButtonShape::Rounded => "rounded",
            ButtonShape::Circle => "circle",
        }
    }

    pub fn attr(self) -> Option<&'static str> {
        (self != ButtonShape::Capsule).then(|| self.id())
    }
}

/// Button. With `href` it becomes a link in the button look (`<a>`).
///
/// `variant`, `size`, `shape` and `disabled` accept fixed values or signals.
///
/// ```ignore
/// view! {
///     <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large>"Next"</Button>
///     <Button shape=ButtonShape::Circle aria_label="Add"><Icon path=icons::PLUS /></Button>
/// }
/// ```
#[component]
pub fn Button(
    #[prop(into, default = ButtonStyle::Bordered.into())] variant: Signal<ButtonStyle>,
    #[prop(into, default = ControlSize::Regular.into())] size: Signal<ControlSize>,
    #[prop(into, default = ButtonShape::Capsule.into())] shape: Signal<ButtonShape>,
    /// `role: .destructive`
    #[prop(into, default = false.into())]
    destructive: Signal<bool>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
    #[prop(optional, into)] href: Option<String>,
    /// Required for buttons without text (e.g. icon only).
    #[prop(optional, into)]
    aria_label: Option<String>,
    #[prop(optional, into)] on_click: Option<Callback<ev::MouseEvent>>,
    children: Children,
) -> impl IntoView {
    let role = move || destructive.get().then_some("destructive");
    let click = move |ev: ev::MouseEvent| {
        if let Some(cb) = on_click {
            cb.run(ev);
        }
    };

    match href {
        Some(href) => view! {
            <a
                class="ui-button"
                href=href
                data-style=move || variant.get().id()
                data-control-size=move || size.get().attr()
                data-shape=move || shape.get().attr()
                data-role=role
                aria-label=aria_label
                aria-disabled=move || disabled.get().then_some("true")
                on:click=click
            >
                {children()}
            </a>
        }
        .into_any(),
        None => view! {
            <button
                class="ui-button"
                type="button"
                data-style=move || variant.get().id()
                data-control-size=move || size.get().attr()
                data-shape=move || shape.get().attr()
                data-role=role
                aria-label=aria_label
                disabled=move || disabled.get()
                on:click=click
            >
                {children()}
            </button>
        }
        .into_any(),
    }
}

/// Link. With `chevron` an arrow appears that moves to the right on hover.
#[component]
pub fn Link(
    #[prop(into)] href: String,
    #[prop(optional)] chevron: bool,
    children: Children,
) -> impl IntoView {
    view! {
        <a class="ui-link" href=href data-variant=chevron.then_some("chevron")>
            {children()}
            {chevron.then(|| view! { <Icon path=icons::CHEVRON_RIGHT size=14 stroke=2.6 /> })}
        </a>
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeVariant {
    /// Akzentfarbe.
    #[default]
    Tint,
    Neutral,
    /// Round counter badge (e.g. unread notifications).
    Count,
}

impl BadgeVariant {
    pub fn attr(self) -> Option<&'static str> {
        match self {
            BadgeVariant::Tint => None,
            BadgeVariant::Neutral => Some("neutral"),
            BadgeVariant::Count => Some("count"),
        }
    }
}

#[component]
pub fn Badge(#[prop(optional)] variant: BadgeVariant, children: Children) -> impl IntoView {
    view! { <span class="ui-badge" data-variant=variant.attr()>{children()}</span> }
}
