//! App-Gerüst nach `NavigationSplitView`: Glas-Sidebar und zwei Panes.

use leptos::{ev, prelude::*};

use crate::{Icon, icons};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitLayout {
    /// Sidebar · schmale Liste · breites Detail.
    #[default]
    ListDetail,
    /// Sidebar · breiter Inhalt · schmale Spalte rechts.
    DetailWide,
}

/// Drei Spalten. Unter 1100 px stapeln sie sich.
#[component]
pub fn SplitView(#[prop(optional)] layout: SplitLayout, children: Children) -> impl IntoView {
    let layout = (layout == SplitLayout::DetailWide).then_some("detail-wide");
    view! { <div class="ui-splitview" data-layout=layout>{children()}</div> }
}

/// Sidebar. Kinder: [`SidebarBrand`], [`SidebarSection`] mit [`SidebarItem`]s, `Spacer`.
#[component]
pub fn Sidebar(#[prop(into)] label: String, children: Children) -> impl IntoView {
    view! { <nav class="ui-sidebar" aria-label=label>{children()}</nav> }
}

/// Logo und Name oben in der Sidebar.
#[component]
pub fn SidebarBrand(#[prop(into)] href: String, children: Children) -> impl IntoView {
    view! {
        <a class="ui-sidebar__brand" href=href>
            <span style="display: inline-flex; color: var(--sys-color-tint)">
                <Icon path=icons::BRAND size=24 />
            </span>
            {children()}
        </a>
    }
}

/// Gruppe von Einträgen, optional mit kleiner Überschrift.
#[component]
pub fn SidebarSection(
    #[prop(optional, into)] label: Option<String>,
    children: Children,
) -> impl IntoView {
    view! {
        <div>
            {label.map(|l| view! { <p class="ui-sidebar__label">{l}</p> })}
            <ul class="ui-sidebar__nav">{children()}</ul>
        </div>
    }
}

/// Eintrag der Sidebar: Icon, Beschriftung und rechts ein Zähler oder beliebiger Inhalt.
#[component]
pub fn SidebarItem(
    #[prop(into)] href: String,
    #[prop(into)] label: Signal<String>,
    /// SVG-Pfad aus [`icons`].
    #[prop(optional)]
    icon: Option<&'static str>,
    #[prop(into, default = false.into())] current: Signal<bool>,
    /// Kurzer Text rechts, z. B. `"3"` oder `"2 aktiv"`.
    #[prop(optional, into)]
    count: Option<Signal<String>>,
    /// Beliebiger Inhalt rechts (z. B. ein `Status`). Hat Vorrang vor `count`.
    #[prop(optional)]
    trailing: Option<AnyView>,
    #[prop(optional, into)] on_click: Option<Callback<ev::MouseEvent>>,
) -> impl IntoView {
    let right = match (trailing, count) {
        (Some(view), _) => Some(view),
        (None, Some(count)) => Some((move || count.get()).into_any()),
        (None, None) => None,
    };
    view! {
        <li>
            <a
                class="ui-sidebar__item"
                href=href
                aria-current=move || current.get().then_some("page")
                on:click=move |ev| {
                    if let Some(cb) = on_click {
                        cb.run(ev);
                    }
                }
            >
                {icon.map(|path| view! { <Icon path=path size=20 /> })}
                <span style="min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
                    {move || label.get()}
                </span>
                {right.map(|r| view! { <span class="ui-sidebar__count">{r}</span> })}
            </a>
        </li>
    }
}

/// Fläche in einer Spalte des [`SplitView`].
#[component]
pub fn Pane(#[prop(into)] label: String, children: Children) -> impl IntoView {
    view! { <section class="ui-pane" aria-label=label>{children()}</section> }
}

/// Kopf eines Panes (Titel, Filter, Status).
#[component]
pub fn PaneHeader(children: Children) -> impl IntoView {
    view! { <div class="ui-pane__header">{children()}</div> }
}

/// Inhalt eines Panes.
#[component]
pub fn PaneBody(children: Children) -> impl IntoView {
    view! { <div class="ui-pane__body">{children()}</div> }
}
