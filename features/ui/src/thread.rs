//! Inbox-Zeile, Kommentar-Blase und mehrzeiliges Textfeld.

use leptos::prelude::*;

use crate::{Avatar, next_id};

/// Zeile einer Inbox. Der Inhalt kommt als Kinder.
#[component]
pub fn InboxRow(
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] on_select: Callback<()>,
    children: Children,
) -> impl IntoView {
    view! {
        <li>
            <button
                class="ui-inbox-row"
                type="button"
                aria-pressed=move || selected.get().to_string()
                on:click=move |_| on_select.run(())
            >
                {children()}
            </button>
        </li>
    }
}

/// Kommentar-Blase. Eigene Kommentare stehen rechts und in der Akzentfarbe.
#[component]
pub fn CommentBubble(
    #[prop(into)] who: String,
    #[prop(into)] initials: String,
    #[prop(into)] time: String,
    #[prop(into)] text: String,
    #[prop(optional)] own: bool,
    /// Der Kommentar stammt vom Agenten.
    #[prop(optional)]
    agent: bool,
) -> impl IntoView {
    view! {
        <div class="ui-comment" data-own=own.then_some("")>
            <Avatar initials=initials agent=agent />
            <div class="ui-comment__bubble">
                <p class="t-footnote t-secondary">{format!("{who} · {time}")}</p>
                <p class="t-body" style="white-space: pre-line">{text}</p>
            </div>
        </div>
    }
}

/// Mehrzeiliges Textfeld. Das Label ist nur für Screenreader sichtbar.
#[component]
pub fn Textarea(
    value: RwSignal<String>,
    #[prop(into)] label: String,
    #[prop(optional, into)] placeholder: Option<String>,
    /// Mindesthöhe, z. B. `"72px"`.
    #[prop(optional, into)]
    min_height: Option<String>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
) -> impl IntoView {
    let id = next_id("textarea");
    view! {
        <label class="ui-sr-only" for=id.clone()>{label}</label>
        <textarea
            class="ui-textarea"
            id=id
            placeholder=placeholder
            style=min_height.map(|h| format!("min-height: {h}"))
            disabled=move || disabled.get()
            prop:value=move || value.get()
            on:input=move |ev| value.set(event_target_value(&ev))
        ></textarea>
    }
}
