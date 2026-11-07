use leptos::prelude::*;

/// Zusätzliche Inline-Styles optional anhängen.
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
    /// Zusätzliches CSS, z. B. `"align-items:center"`.
    #[prop(optional, into)]
    css: Option<String>,
    children: Children,
) -> impl IntoView {
    let style = style_of(&[spacing.map(|s| format!("--spacing:{s}")), css]);
    view! { <div class="ui-vstack" style=style>{children()}</div> }
}

/// Horizontaler Stapel (`HStack`), bricht bei Platzmangel um.
#[component]
pub fn HStack(
    #[prop(optional, into)] spacing: Option<String>,
    #[prop(optional, into)] css: Option<String>,
    children: Children,
) -> impl IntoView {
    let style = style_of(&[spacing.map(|s| format!("--spacing:{s}")), css]);
    view! { <div class="ui-hstack" style=style>{children()}</div> }
}

/// Übereinanderliegende Ebenen (`ZStack`) – Kinder teilen sich eine Grid-Zelle.
#[component]
pub fn ZStack(#[prop(optional, into)] css: Option<String>, children: Children) -> impl IntoView {
    view! { <div class="ui-zstack" style=css>{children()}</div> }
}

/// Füllt freien Platz in einem `HStack`.
#[component]
pub fn Spacer() -> impl IntoView {
    view! { <div class="ui-spacer"></div> }
}

/// Raster mit `cols` Spalten (fällt unter 1024 px auf 2, unter 640 px auf 1).
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

/// Zwei Spalten, z. B. Text + Bild. Stapelt sich auf Tablet und Phone.
#[component]
pub fn Split(children: Children) -> impl IntoView {
    view! { <div class="ui-split">{children()}</div> }
}

/// Zentrierter Inhaltsbereich mit maximaler Breite.
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SectionTone {
    #[default]
    Plain,
    Grouped,
    Secondary,
}

/// Abschnitt einer Webseite mit großem vertikalem Abstand.
#[component]
pub fn PageSection(
    #[prop(optional)] tone: SectionTone,
    #[prop(optional, into)] id: Option<String>,
    #[prop(optional, into)] css: Option<String>,
    children: Children,
) -> impl IntoView {
    let tone = match tone {
        SectionTone::Plain => None,
        SectionTone::Grouped => Some("grouped"),
        SectionTone::Secondary => Some("secondary"),
    };
    view! {
        <section class="ui-page-section" data-tone=tone id=id style=css>
            {children()}
        </section>
    }
}

/// Glas-Fläche (Liquid Glass). Braucht Inhalt dahinter, um zu wirken.
#[component]
pub fn Glass(#[prop(optional, into)] css: Option<String>, children: Children) -> impl IntoView {
    view! { <div class="ui-glass" style=css>{children()}</div> }
}
