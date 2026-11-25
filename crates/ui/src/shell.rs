//! App scaffold after `NavigationSplitView`: glass sidebar and two panes.

use leptos::wasm_bindgen::JsCast;
use leptos::{ev, prelude::*};

use crate::{Button, ButtonShape, ButtonStyle, Icon, StackLevel, icons, is_back_swipe};

thread_local! {
    /// The level the last [`SplitView`] with levels showed.
    static LAST_LEVEL: std::cell::Cell<Option<StackLevel>> = const { std::cell::Cell::new(None) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitLayout {
    /// Sidebar · narrow list · wide detail.
    #[default]
    ListDetail,
    /// Sidebar · wide content · narrow column on the right.
    DetailWide,
}

/// Up to three columns (list or picker, detail, context), each as tall as the viewport and
/// scrolling on its own. Up to 1280 px the third column moves below the second one.
///
/// Below 768 px the page works like an iOS navigation stack: if `level` is given, the list
/// pane (first pane) and the detail panes (all following panes) are two levels and only the one
/// `level` selects is on screen; going deeper slides the detail in from the right. The level
/// should come from the address of the page. On the detail level a round glass button floats
/// top left over the content (no bar; the content starts below it and scrolls under it) and links
/// to `back_href`; `back_label` is its tooltip. A swipe from the left edge does the same. From
/// 768 px on, all columns stand side by side and none of this is shown.
#[component]
pub fn SplitView(
    #[prop(optional)] layout: SplitLayout,
    #[prop(optional, into)] level: Option<Signal<StackLevel>>,
    /// Name of the list level, the tooltip of the back button ("Posteingang").
    #[prop(optional, into)]
    back_label: Option<String>,
    /// Address of the list level.
    #[prop(optional, into)]
    back_href: Option<String>,
    children: Children,
) -> impl IntoView {
    let layout = (layout == SplitLayout::DetailWide).then_some("detail-wide");
    let attr = move || level.map(|l| l.get().attr());
    // The slide only runs after a change, never when the page is first shown.
    let motion = RwSignal::new(None::<&'static str>);
    // A page that is rebuilt per entry (a new `SplitView`) still slides: the last level that any
    // `SplitView` showed counts as the previous one.
    Effect::new(move |previous: Option<StackLevel>| {
        let Some(level) = level else {
            return StackLevel::List;
        };
        let now = level.get();
        if let Some(previous) = previous.or_else(|| LAST_LEVEL.get()) {
            motion.set(previous.motion_to(now));
        }
        LAST_LEVEL.set(Some(now));
        now
    });
    let touch = StoredValue::new(None::<(f64, f64)>);
    let on_start = move |event: ev::TouchEvent| {
        let first = event
            .touches()
            .get(0)
            .map(|t| (t.client_x() as f64, t.client_y() as f64));
        touch.set_value(first);
    };
    let back_target = back_href.clone();
    let on_end = move |event: ev::TouchEvent| {
        let started = touch.get_value();
        touch.set_value(None);
        let (Some((x0, y0)), Some(end), Some(target)) = (
            started,
            event.changed_touches().get(0),
            back_target.as_deref(),
        ) else {
            return;
        };
        let on_detail = level.is_some_and(|l| l.get_untracked() == StackLevel::Detail);
        if on_detail && is_back_swipe(x0, y0, end.client_x() as f64, end.client_y() as f64) {
            let _ = window().location().set_hash(target);
        }
    };
    let back = level.map(|_| {
        view! {
            <nav class="ui-splitview__back" aria-label=crate::t!(UiBack)>
                <a
                    class="ui-splitview__back-link"
                    href=back_href.clone()
                    aria-label=crate::t!(UiBack)
                    title=back_label
                >
                    <span aria-hidden="true">"‹"</span>
                </a>
            </nav>
        }
    });
    view! {
        <div
            class="ui-splitview"
            data-layout=layout
            data-level=attr
            data-motion=move || motion.get()
            on:touchstart=on_start
            on:touchend=on_end
        >
            {back}
            {children()}
        </div>
    }
}

/// Logo and name at the top of the sidebar.
#[component]
pub fn SidebarBrand(#[prop(into)] href: String, children: Children) -> impl IntoView {
    view! {
        <a class="ui-sidebar__brand" href=href>
            <span class="ui-icon-tint">
                <Icon path=icons::BRAND size=24 />
            </span>
            {children()}
        </a>
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

/// Floating navigation pill ("liquid glass") at the bottom edge, centred, on every screen width.
///
/// Holds a few fixed main targets ([`BottomNavItem`] as children, `slots` in total including
/// "more") and a last button that opens a bottom sheet with everything else (`more`). Nothing
/// scrolls sideways. A glass capsule slides to the slot `active` names (counted from 0, "more"
/// is the last one; `None`: none is current). The button is marked as current when `more_current` is true (the current
/// page lives in the sheet) and carries `more_badge` (summed attention of the hidden targets).
///
/// The sheet is a modal dialog: focus moves into it, Tab stays inside, Escape or a tap outside
/// closes it and focus returns to the button; the page behind does not scroll. Following a link in
/// the sheet closes it.
#[component]
pub fn BottomNav(
    #[prop(into)] label: String,
    /// Label of the button that opens the sheet, e.g. "Mehr".
    #[prop(into)]
    more_label: String,
    /// Title of the sheet.
    #[prop(into)]
    more_title: String,
    /// The current page is one of the targets in the sheet.
    #[prop(into, default = false.into())]
    more_current: Signal<bool>,
    /// Number of new things behind the button (0: no badge).
    #[prop(into, default = 0.into())]
    more_badge: Signal<usize>,
    /// Opaque pill without blur (the fallback for `prefers-reduced-transparency` and browsers
    /// without `backdrop-filter`, shown on purpose, e.g. in a style guide).
    #[prop(optional)]
    solid: bool,
    /// Number of slots including "more" (the pill is as wide as its slots).
    #[prop(default = 5)]
    slots: usize,
    /// Index of the slot of the current page (the capsule slides there).
    #[prop(into, default = None.into())]
    active: Signal<Option<usize>>,
    /// Content of the sheet (a list of [`SidebarItem`]s, further sections).
    #[prop(into)]
    more: ViewFn,
    /// The main targets.
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let away = crate::autohide::use_autohide();
    let sheet = NodeRef::<leptos::html::Div>::new();
    let button = NodeRef::<leptos::html::Button>::new();
    let more_label = StoredValue::new(more_label);
    let more_title = StoredValue::new(more_title);

    // Close, give the page its scroll back and the focus to the button.
    let close = move || {
        if open.get_untracked() {
            open.set(false);
            if let Some(b) = button.get_untracked() {
                let _ = b.focus();
            }
        }
    };
    // Lock the page behind the sheet; always release it, also if the nav disappears.
    Effect::new(move |_| {
        let on = open.get();
        if let Some(body) = document().body() {
            let _ = body.class_list().toggle_with_force("ui-scroll-locked", on);
        }
        if on {
            // After the sheet became visible: focus the first target.
            request_animation_frame(move || {
                if let Some(sheet) = sheet.get_untracked()
                    && let Ok(Some(first)) = sheet.query_selector("a[href], button")
                    && let Ok(el) = first.dyn_into::<web_sys::HtmlElement>()
                {
                    let _ = el.focus();
                }
            });
        }
    });
    on_cleanup(move || {
        if let Some(body) = document().body() {
            let _ = body.class_list().remove_1("ui-scroll-locked");
        }
    });
    // Back/forward or a link elsewhere on the page leaves the sheet.
    let hash_listener = window_event_listener(ev::hashchange, move |_| open.set(false));
    on_cleanup(move || hash_listener.remove());

    let on_keydown = move |ev: ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            ev.prevent_default();
            close();
        }
        // Keep Tab inside the dialog.
        "Tab" => {
            let Some(sheet) = sheet.get_untracked() else {
                return;
            };
            let Ok(list) = sheet.query_selector_all("a[href], button:not([disabled])") else {
                return;
            };
            let last = list.length().saturating_sub(1);
            let (Some(first_el), Some(last_el)) = (list.item(0), list.item(last)) else {
                return;
            };
            let active = document().active_element();
            let is = |n: &web_sys::Node| active.as_ref().is_some_and(|a| a.is_same_node(Some(n)));
            if ev.shift_key() && is(&first_el) {
                ev.prevent_default();
                if let Ok(el) = last_el.dyn_into::<web_sys::HtmlElement>() {
                    let _ = el.focus();
                }
            } else if !ev.shift_key() && is(&last_el) {
                ev.prevent_default();
                if let Ok(el) = first_el.dyn_into::<web_sys::HtmlElement>() {
                    let _ = el.focus();
                }
            }
        }
        _ => {}
    };
    // A tap on a link inside the sheet follows it and closes the sheet.
    let on_sheet_click = move |ev: ev::MouseEvent| {
        let on_link = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| el.closest("a[href]").ok().flatten().is_some());
        if on_link {
            open.set(false);
        }
    };

    view! {
        <nav
            class="ui-bottomnav"
            aria-label=label
            data-solid=solid.then_some("")
            data-hidden=move || away.get().then_some("")
        >
          <div class="ui-bottomnav__pill" style=format!("--slots:{slots}")>
            <ul
                class="ui-bottomnav__list"
                style=move || format!("--active:{}", active.get().unwrap_or(0))
                data-active=move || active.get().is_some().then_some("true")
            >
                {children()}
                <li>
                    <button
                        type="button"
                        node_ref=button
                        class="ui-bottomnav__item"
                        aria-haspopup="dialog"
                        aria-expanded=move || if open.get() { "true" } else { "false" }
                        aria-current=move || more_current.get().then_some("page")
                        on:click=move |_| open.update(|o| *o = !*o)
                    >
                        <span class="ui-bottomnav__icon">
                            <Icon path=icons::MORE size=22 />
                            <BottomNavBadge count=more_badge />
                        </span>
                        <span class="ui-bottomnav__label">{more_label.get_value()}</span>
                    </button>
                </li>
            </ul>
          </div>
        </nav>
        <div
            class="ui-bottomnav-overlay"
            hidden=move || !open.get()
            on:click=move |ev| {
                // Only a tap on the dimmed area itself, not on the sheet.
                if ev.target() == ev.current_target() {
                    close();
                }
            }
        >
            <div
                class="ui-bottomnav-sheet"
                node_ref=sheet
                role="dialog"
                aria-modal="true"
                aria-label=more_title.get_value()
                on:keydown=on_keydown
                on:click=on_sheet_click
            >
                <div class="ui-sheet__grabber" aria-hidden="true"></div>
                <div class="ui-bottomnav-sheet__head">
                    <p class="t-headline">{more_title.get_value()}</p>
                    <Button
                        variant=ButtonStyle::Glass
                        shape=ButtonShape::Circle
                        aria_label=crate::t!(UiClose)
                        on_click=move |_| close()
                    >
                        <Icon path=icons::CLOSE size=16 stroke=2.6 />
                    </Button>
                </div>
                <div class="ui-bottomnav-sheet__body">{more.run()}</div>
            </div>
        </div>
    }
}

/// Small counter on an icon of the [`BottomNav`]. Invisible at 0; reads as "N neu" to screen readers.
#[component]
fn BottomNavBadge(#[prop(into)] count: Signal<usize>) -> impl IntoView {
    let shown = move || count.get() > 0;
    view! {
        <Show when=shown>
            <span class="ui-bottomnav__badge" aria-hidden="true">
                {move || if count.get() > 99 { "99+".to_string() } else { count.get().to_string() }}
            </span>
            <span class="ui-sr-only">{move || crate::t!(UiNewCount, n = count.get())}</span>
        </Show>
    }
}

/// Main target in the [`BottomNav`]: icon, short label and an optional counter.
#[component]
pub fn BottomNavItem(
    #[prop(into)] href: String,
    #[prop(into)] label: Signal<String>,
    /// SVG path from [`icons`].
    icon: &'static str,
    #[prop(into, default = false.into())] current: Signal<bool>,
    /// Number of new things (0: no badge).
    #[prop(into, default = 0.into())]
    badge: Signal<usize>,
) -> impl IntoView {
    view! {
        <li>
            <a
                class="ui-bottomnav__item"
                href=href
                aria-current=move || current.get().then_some("page")
            >
                <span class="ui-bottomnav__icon">
                    <Icon path=icon size=22 />
                    <BottomNavBadge count=badge />
                </span>
                <span class="ui-bottomnav__label">{move || label.get()}</span>
            </a>
        </li>
    }
}

/// Column of the [`SplitView`] that picks the item the next column shows (a pull request, an
/// agent run). Up to 1100 px it becomes a chip strip above the content.
#[component]
pub fn PickerPane(
    #[prop(into)] label: String,
    /// Small heading above the entries.
    #[prop(into)]
    title: String,
    children: Children,
) -> impl IntoView {
    view! {
        <section class="ui-pane" data-kind="picker" aria-label=label>
            <div class="ui-pane__header">
                <h2 class="t-headline">{title}</h2>
            </div>
            <div class="ui-pane__body">{children()}</div>
        </section>
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
