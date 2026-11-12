use leptos::prelude::*;

use crate::{Icon, icons};

/// Link in der [`Navbar`].
#[derive(Clone, Debug)]
pub struct NavLink {
    pub label: &'static str,
    pub href: &'static str,
    /// Markiert die aktuelle Seite (`aria-current="page"`).
    pub current: bool,
}

/// Schwebende Glas-Navigationsleiste für Webseiten.
///
/// Die Kinder erscheinen rechts (Aktionen wie Suche oder Kontakt-Button).
#[component]
pub fn Navbar(
    #[prop(into)] brand: String,
    links: Vec<NavLink>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <nav class="ui-navbar" aria-label="Hauptnavigation">
            <div class="ui-navbar__inner">
                <a class="ui-navbar__brand" href="#">
                    <Icon path=icons::BRAND size=22 />
                    {brand}
                </a>
                <ul class="ui-navbar__links">
                    {links
                        .into_iter()
                        .map(|l| {
                            view! {
                                <li>
                                    <a
                                        class="ui-navbar__link"
                                        href=l.href
                                        aria-current=l.current.then_some("page")
                                    >
                                        {l.label}
                                    </a>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
                <div class="ui-hstack" style="--spacing: 6px">{children.map(|c| c())}</div>
            </div>
        </nav>
    }
}

/// iOS-Navigation mit großem Titel. Die Kinder bilden die obere Leiste
/// (z. B. Zurück-Button links, Aktionen rechts).
#[component]
pub fn IosNav(#[prop(into)] title: Signal<String>, children: Children) -> impl IntoView {
    view! {
        <div class="ui-iosnav" style="padding: 0 0 8px">
            <div class="ui-iosnav__bar">{children()}</div>
            <h1 class="ui-iosnav__title" style="padding-inline: 4px">{move || title.get()}</h1>
        </div>
    }
}

/// Eintrag der [`TabBar`].
#[derive(Clone, Copy, Debug)]
pub struct TabItem {
    pub id: &'static str,
    pub label: &'static str,
    /// SVG-Pfad, siehe [`icons`].
    pub icon: &'static str,
}

/// Schwebende Tab-Leiste (Kapsel). `selected` enthält die ID des aktiven Tabs.
#[component]
pub fn TabBar(items: &'static [TabItem], selected: RwSignal<&'static str>) -> impl IntoView {
    view! {
        <div class="ui-tabbar">
            {items
                .iter()
                .map(|&item| {
                    view! {
                        <button
                            class="ui-tabbar__item"
                            type="button"
                            aria-current=move || {
                                if selected.get() == item.id { "page" } else { "false" }
                            }
                            on:click=move |_| selected.set(item.id)
                        >
                            <Icon path=item.icon size=24 />
                            <span>{item.label}</span>
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}
