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
///
/// `detail_open` only matters on those narrow screens: if given, the list pane (first pane) and
/// the detail pane (second pane) become two levels and only the one the signal selects is shown
/// (`true`: the detail). Wide screens always show both.
#[component]
pub fn SplitView(
    #[prop(optional)] layout: SplitLayout,
    #[prop(optional, into)] detail_open: Option<Signal<bool>>,
    children: Children,
) -> impl IntoView {
    let layout = (layout == SplitLayout::DetailWide).then_some("detail-wide");
    let level = move || detail_open.map(|open| if open.get() { "detail" } else { "list" });
    view! {
        <div class="ui-splitview" data-layout=layout data-level=level>
            {children()}
        </div>
    }
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

/// What a sidebar group is for. Matters on narrow screens (up to 1100 px), where the sidebar
/// dissolves: the primary navigation moves into the [`BottomNav`], pickers become a strip above
/// the content and everything else follows below the content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SectionRole {
    /// Settings-like content (rules, connections). Below the content on narrow screens.
    #[default]
    Default,
    /// Page navigation. Hidden on narrow screens because the [`BottomNav`] shows the same entries.
    Primary,
    /// Selects the item the page shows (e.g. a pull request). A scrollable strip above the content.
    Picker,
}

impl SectionRole {
    fn attr(self) -> Option<&'static str> {
        match self {
            SectionRole::Default => None,
            SectionRole::Primary => Some("primary"),
            SectionRole::Picker => Some("picker"),
        }
    }
}

/// Group of entries, optionally with a small heading.
#[component]
pub fn SidebarSection(
    #[prop(optional, into)] label: Option<String>,
    #[prop(optional)] role: SectionRole,
    children: Children,
) -> impl IntoView {
    view! {
        <div data-role=role.attr()>
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
                <span class="ui-sidebar__label-text">{move || label.get()}</span>
                {right.map(|r| view! { <span class="ui-sidebar__count">{r}</span> })}
            </a>
        </li>
    }
}

/// Navigation bar fixed to the bottom edge on narrow screens (up to 1100 px); hidden above that,
/// where the [`Sidebar`] shows the same entries. Children: [`SidebarItem`]s. Wide content scrolls
/// sideways inside the bar. Respects the safe areas of notched devices.
#[component]
pub fn BottomNav(#[prop(into)] label: String, children: Children) -> impl IntoView {
    view! {
        <nav class="ui-bottomnav" aria-label=label>
            <ul class="ui-bottomnav__list">{children()}</ul>
        </nav>
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
