use leptos::prelude::*;

use crate::{Icon, icons};

/// Link in the [`Navbar`].
#[derive(Clone, Debug)]
pub struct NavLink {
    pub label: &'static str,
    pub href: &'static str,
    /// Marks the current page (`aria-current="page"`).
    pub current: bool,
}

/// Floating glass navigation bar for web pages.
///
/// The children appear on the right (actions such as search or contact button).
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

/// iOS navigation with a large title. The children form the top bar
/// (e.g. back button on the left, actions on the right).
#[component]
pub fn IosNav(#[prop(into)] title: Signal<String>, children: Children) -> impl IntoView {
    view! {
        <div class="ui-iosnav" style="padding: 0 0 8px">
            <div class="ui-iosnav__bar">{children()}</div>
            <h1 class="ui-iosnav__title" style="padding-inline: 4px">{move || title.get()}</h1>
        </div>
    }
}

/// Entry of the [`TabBar`].
#[derive(Clone, Copy, Debug)]
pub struct TabItem {
    pub id: &'static str,
    pub label: &'static str,
    /// SVG-Pfad, siehe [`icons`].
    pub icon: &'static str,
}

/// Floating tab bar (capsule). `selected` contains the ID of the active tab.
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
