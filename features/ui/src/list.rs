use leptos::{ev, prelude::*};

use crate::{Icon, Tint, icons};

/// Container für mehrere Sektionen (`List` mit `.insetGrouped`).
#[component]
pub fn List(children: Children) -> impl IntoView {
    view! { <div class="ui-list">{children()}</div> }
}

/// Eine Sektion mit optionaler Kopf- und Fußzeile.
#[component]
pub fn ListSection(
    #[prop(optional, into)] header: Option<String>,
    #[prop(optional, into)] footer: Option<String>,
    /// Zeilen haben farbige Icons – Trennlinien beginnen hinter dem Icon.
    #[prop(optional)]
    icons: bool,
    /// Große, fette Überschrift (z. B. Titel eines Onboarding-Schritts).
    #[prop(optional)]
    prominent: bool,
    /// Hellere Fläche für Listen, die schon auf einer Karte oder in einem `Pane` liegen.
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
                style=nested.then_some("background: var(--sys-color-tertiary-grouped-background)")
            >
                {children()}
            </ul>
            {footer.map(|f| view! { <p class="ui-list__footer">{f}</p> })}
        </div>
    }
}

/// Zeile einer Liste: Titel, optional Untertitel, Detail, Icon und Chevron.
///
/// * mit `href` wird die Zeile ein Link,
/// * mit `toggle_row` ein `<label>` (Klick auf die Zeile schaltet den enthaltenen `Toggle`),
/// * sonst ein `<div>`.
///
/// Zusätzlicher Inhalt am Zeilenende (z. B. `Toggle`, `Badge`) kommt als Kinder.
#[component]
pub fn ListRow(
    #[prop(into)] title: String,
    #[prop(optional, into)] subtitle: Option<String>,
    /// Detail rechts neben dem Titel – darf ein Signal sein.
    #[prop(optional, into)]
    detail: Option<Signal<String>>,
    /// Farbe und Pfad des Icons.
    #[prop(optional)]
    icon: Option<(Tint, &'static str)>,
    #[prop(optional)] chevron: bool,
    #[prop(optional, into)] href: Option<String>,
    #[prop(optional)] toggle_row: bool,
    /// Lässt den Inhalt bei Platzmangel umbrechen.
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

    let wrap_css = wrap.then_some("flex-wrap: wrap");
    let row = match (href, toggle_row) {
        (Some(href), _) => {
            view! { <a class="ui-list__row" href=href style=wrap_css>{body}</a> }.into_any()
        }
        (None, true) => {
            view! { <label class="ui-list__row" style="cursor: pointer">{body}</label> }.into_any()
        }
        (None, false) => view! { <div class="ui-list__row" style=wrap_css>{body}</div> }.into_any(),
    };
    view! { <li>{row}</li> }
}

/// Zentrierte Aktionszeile, z. B. „Abmelden“.
#[component]
pub fn ListButtonRow(
    #[prop(into)] title: String,
    #[prop(optional)] destructive: bool,
    #[prop(optional, into)] on_click: Option<Callback<ev::MouseEvent>>,
) -> impl IntoView {
    view! {
        <li>
            <button
                class="ui-list__row"
                type="button"
                style=format!(
                    "justify-content: center;{}",
                    if destructive { " color: var(--sys-color-red)" } else { "" },
                )
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
