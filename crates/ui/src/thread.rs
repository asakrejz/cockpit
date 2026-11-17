//! Inbox row, comment bubble and multi-line text field.

use leptos::prelude::*;

use crate::{Avatar, RichText, next_id};

/// Row of an inbox. The content comes as children.
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

/// Comment bubble. Own comments are on the right and in the accent color.
#[component]
pub fn CommentBubble(
    #[prop(into)] who: String,
    #[prop(into)] initials: String,
    #[prop(into)] time: String,
    #[prop(into)] text: String,
    /// Formatted text; if not empty, it is shown instead of `text`.
    #[prop(optional)]
    rich: cockpit_core::adf::Doc,
    #[prop(optional)] own: bool,
    /// The comment comes from the agent.
    #[prop(optional)]
    agent: bool,
) -> impl IntoView {
    view! {
        <div class="ui-comment" data-own=own.then_some("")>
            <Avatar initials=initials agent=agent />
            <div class="ui-comment__bubble">
                <p class="t-footnote t-secondary">{format!("{who} · {time}")}</p>
                {match rich {
                    doc if !doc.is_empty() => view! { <RichText doc=doc /> }.into_any(),
                    _ => view! { <p class="t-body" style="white-space: pre-line">{text}</p> }.into_any(),
                }}
            </div>
        </div>
    }
}

/// Multi-line text field. The label is only visible to screen readers.
#[component]
pub fn Textarea(
    value: RwSignal<String>,
    #[prop(into)] label: String,
    #[prop(optional, into)] placeholder: Option<String>,
    /// Minimum height, e.g. `"72px"`.
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
