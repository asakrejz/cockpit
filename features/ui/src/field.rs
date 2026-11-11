use leptos::prelude::*;

use crate::{Icon, icons, next_id};

/// Textfeld mit Label und optionalem Hinweis. Mit `error` wird es rot markiert.
#[component]
pub fn TextField(
    #[prop(into)] label: String,
    value: RwSignal<String>,
    #[prop(optional, into)] placeholder: Option<String>,
    /// HTML-`type`, z. B. `"email"`, `"password"`, `"number"`.
    #[prop(default = "text")]
    input_type: &'static str,
    /// Hinweis oder Fehlermeldung unter dem Feld.
    #[prop(optional, into)]
    hint: Option<String>,
    #[prop(into, default = false.into())] error: Signal<bool>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
) -> impl IntoView {
    let id = next_id("field");
    view! {
        <div class="ui-field" data-state=move || error.get().then_some("error")>
            <label class="ui-field__label" for=id.clone()>{label}</label>
            <input
                class="ui-field__input"
                id=id
                type=input_type
                placeholder=placeholder
                disabled=move || disabled.get()
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
            {hint.map(|h| view! { <span class="ui-field__hint">{h}</span> })}
        </div>
    }
}

/// Suchfeld mit Lupe.
#[component]
pub fn SearchField(
    value: RwSignal<String>,
    #[prop(default = "Suchen")] placeholder: &'static str,
) -> impl IntoView {
    let id = next_id("search");
    view! {
        <div class="ui-search">
            <Icon path=icons::SEARCH size=17 stroke=2.2 />
            <label class="ui-sr-only" for=id.clone()>{placeholder}</label>
            <input
                class="ui-field__input"
                id=id
                type="search"
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
        </div>
    }
}
