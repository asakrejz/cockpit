use leptos::prelude::*;

/// Hell oder Dunkel (`colorScheme`).
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

/// Akzentfarbe (`accentColor`). Eine Variable, alles andere wird abgeleitet.
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

    /// Name der System-Farbe, z. B. `--sys-color-<id>`.
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

/// "Kontrast erhöhen" (Accessibility).
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

/// Wurzel für alle Komponenten. Setzt Theme, Tint und Kontrast für den Teilbaum.
///
/// Die Tokens werden am `data-theme`-Element neu aufgelöst – deshalb muss das
/// Attribut immer gesetzt sein, auch im Hell-Modus.
#[component]
pub fn Root(
    #[prop(into, default = Theme::Light.into())] theme: Signal<Theme>,
    #[prop(into, default = Tint::Blue.into())] tint: Signal<Tint>,
    #[prop(into, default = Contrast::Normal.into())] contrast: Signal<Contrast>,
    /// Zusätzliche Klassen (z. B. für das Layout der Hülle).
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
