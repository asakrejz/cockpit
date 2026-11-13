//! App scaffold after `NavigationSplitView`: glass sidebar and two panes.

use leptos::{ev, prelude::*};

use crate::{Icon, icons};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitLayout {
    /// Sidebar · narrow list · wide detail.
    #[default]
    ListDetail,
    /// Sidebar · wide content · narrow column on the right.
    DetailWide,
}

/// Three columns. Below 1100 px they stack.
#[component]
pub fn SplitView(#[prop(optional)] layout: SplitLayout, children: Children) -> impl IntoView {
    let layout = (layout == SplitLayout::DetailWide).then_some("detail-wide");
    view! { <div class="ui-splitview" data-layout=layout>{children()}</div> }
}

/// Sidebar. Children: [`SidebarBrand`], [`SidebarSection`] with [`SidebarItem`]s, `Spacer`.
#[component]
pub fn Sidebar(#[prop(into)] label: String, children: Children) -> impl IntoView {
    view! { <nav class="ui-sidebar" aria-label=label>{children()}</nav> }
}

/// Logo and name at the top of the sidebar.
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

/// Group of entries, optionally with a small heading.
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

/// Entry of the sidebar: icon, label and on the right a counter or any content.
#[component]
pub fn SidebarItem(
    #[prop(into)] href: String,
    #[prop(into)] label: Signal<String>,
    /// SVG path from [`icons`].
    #[prop(optional)]
    icon: Option<&'static str>,
    #[prop(into, default = false.into())] current: Signal<bool>,
    /// Short text on the right, e.g. `"3"` or `"2 aktiv"`.
    #[prop(optional, into)]
    count: Option<Signal<String>>,
    /// Any content on the right (e.g. a `Status`). Takes precedence over `count`.
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

/// Surface in a column of the [`SplitView`].
#[component]
pub fn Pane(#[prop(into)] label: String, children: Children) -> impl IntoView {
    view! { <section class="ui-pane" aria-label=label>{children()}</section> }
}

/// Head of a pane (title, filter, status).
#[component]
pub fn PaneHeader(children: Children) -> impl IntoView {
    view! { <div class="ui-pane__header">{children()}</div> }
}

/// Content of a pane.
#[component]
pub fn PaneBody(children: Children) -> impl IntoView {
    view! { <div class="ui-pane__body">{children()}</div> }
}
