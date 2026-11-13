use leptos::prelude::*;

/// Light or dark (`colorScheme`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Light, Theme::Dark];

    pub fn id(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// Accent color (`accentColor`). One variable, everything else is derived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tint {
    #[default]
    Blue,
    Indigo,
    Purple,
    Pink,
    Red,
    Orange,
    Green,
    Teal,
}

impl Tint {
    pub const ALL: [Tint; 8] = [
        Tint::Blue,
        Tint::Indigo,
        Tint::Purple,
        Tint::Pink,
        Tint::Red,
        Tint::Orange,
        Tint::Green,
        Tint::Teal,
    ];

    /// Name of the system color, e.g. `--sys-color-<id>`.
    pub fn id(self) -> &'static str {
        match self {
            Tint::Blue => "blue",
            Tint::Indigo => "indigo",
            Tint::Purple => "purple",
            Tint::Pink => "pink",
            Tint::Red => "red",
            Tint::Orange => "orange",
            Tint::Green => "green",
            Tint::Teal => "teal",
        }
    }
}

/// "Increase contrast" (accessibility).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Contrast {
    #[default]
    Normal,
    High,
}

impl Contrast {
    /// `None` = Attribut weglassen.
    pub fn attr(self) -> Option<&'static str> {
        match self {
            Contrast::Normal => None,
            Contrast::High => Some("high"),
        }
    }
}

/// Root for all components. Sets theme, tint and contrast for the subtree.
///
/// The tokens are resolved anew at the `data-theme` element – therefore the
/// attribute must always be set, also in light mode.
#[component]
pub fn Root(
    #[prop(into, default = Theme::Light.into())] theme: Signal<Theme>,
    #[prop(into, default = Tint::Blue.into())] tint: Signal<Tint>,
    #[prop(into, default = Contrast::Normal.into())] contrast: Signal<Contrast>,
    /// Additional classes (e.g. for the layout of the shell).
    #[prop(optional, into)]
    class: String,
    children: Children,
) -> impl IntoView {
    view! {
        <div
            class=format!("ui-root {class}")
            data-theme=move || theme.get().id()
            data-contrast=move || contrast.get().attr()
            style=move || format!("--sys-color-tint: var(--sys-color-{});", tint.get().id())
        >
            {children()}
        </div>
    }
}
