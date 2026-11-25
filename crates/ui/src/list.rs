use leptos::{ev, prelude::*};

use crate::{Icon, Tint, icons};

/// Container for several sections (`List` with `.insetGrouped`).
#[component]
pub fn List(children: Children) -> impl IntoView {
    view! { <div class="ui-list">{children()}</div> }
}

/// A section with optional header and footer.
#[component]
pub fn ListSection(
    #[prop(optional, into)] header: Option<String>,
    #[prop(optional, into)] footer: Option<String>,
    /// Rows have colored icons – separators start behind the icon.
    #[prop(optional)]
    icons: bool,
    /// Large, bold heading (e.g. title of an onboarding step).
    #[prop(optional)]
    prominent: bool,
    /// Lighter surface for lists that already lie on a card or in a `Pane`.
    #[prop(optional)]
    nested: bool,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="ui-list__section">
            {header
                .map(|h| {
                    view! { <p class="ui-list__header" data-prominent=prominent.then_some("")>{h}</p> }
                })}
            <ul
                class="ui-list__group"
                data-icons=icons.then_some("")
                data-nested=nested.then_some("")
            >
                {children()}
            </ul>
            {footer.map(|f| view! { <p class="ui-list__footer">{f}</p> })}
        </div>
    }
}

/// Row of a list: title, optional subtitle, detail, icon and chevron.
///
/// * with `href` the row becomes a link,
/// * with `toggle_row` a `<label>` (click on the row switches the contained `Toggle`),
/// * otherwise a `<div>`.
///
/// Additional content at the row end (e.g. `Toggle`, `Badge`) comes as children.
#[component]
pub fn ListRow(
    #[prop(into)] title: String,
    #[prop(optional, into)] subtitle: Option<String>,
    /// Detail to the right of the title – may be a signal.
    #[prop(optional, into)]
    detail: Option<Signal<String>>,
    /// Color and path of the icon.
    #[prop(optional)]
    icon: Option<(Tint, &'static str)>,
    #[prop(optional)] chevron: bool,
    #[prop(optional, into)] href: Option<String>,
    #[prop(optional)] toggle_row: bool,
    /// Lets the content wrap when there is not enough room.
    #[prop(optional)]
    wrap: bool,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let body = view! {
        {icon
            .map(|(tint, path)| {
                view! {
                    <span
                        class="ui-list__icon"
                        style=format!("background: var(--sys-color-{})", tint.id())
                        aria-hidden="true"
                    >
                        <Icon path=path size=18 />
                    </span>
                }
            })}
        <span class="ui-list__title">
            {title}
            {subtitle.map(|s| view! { <span class="ui-list__subtitle">{s}</span> })}
        </span>
        {detail.map(|d| view! { <span class="ui-list__detail">{move || d.get()}</span> })}
        {children.map(|c| c())}
        {chevron
            .then(|| {
                view! {
                    <span class="ui-list__chevron">
                        <Icon path=icons::CHEVRON_ROW size=14 stroke=3.0 />
                    </span>
                }
            })}
    };

    let row = match (href, toggle_row) {
        (Some(href), _) => {
            view! { <a class="ui-list__row" class:ui-flex-wrap=wrap href=href>{body}</a> }
                .into_any()
        }
        (None, true) => view! { <label class="ui-list__row">{body}</label> }.into_any(),
        (None, false) => {
            view! { <div class="ui-list__row" class:ui-flex-wrap=wrap>{body}</div> }.into_any()
        }
    };
    view! { <li>{row}</li> }
}

/// Centered action row, e.g. „Abmelden“.
#[component]
pub fn ListButtonRow(
    #[prop(into)] title: String,
    #[prop(optional)] destructive: bool,
    #[prop(optional, into)] on_click: Option<Callback<ev::MouseEvent>>,
) -> impl IntoView {
    view! {
        <li>
            <button
                class="ui-list__row ui-justify-center"
                type="button"
                data-role=destructive.then_some("destructive")
                on:click=move |ev| {
                    if let Some(cb) = on_click {
                        cb.run(ev);
                    }
                }
            >
                {title}
            </button>
        </li>
    }
}
