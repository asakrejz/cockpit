use leptos::prelude::*;

use crate::{Icon, icons, next_id};

/// Text field with label and optional hint. With `error` it is marked red.
#[component]
pub fn TextField(
    #[prop(into)] label: String,
    value: RwSignal<String>,
    #[prop(optional, into)] placeholder: Option<String>,
    /// HTML-`type`, z. B. `"email"`, `"password"`, `"number"`.
    #[prop(default = "text")]
    input_type: &'static str,
    /// Hint or error message below the field.
    #[prop(optional, into)]
    hint: Option<String>,
    #[prop(into, default = false.into())] error: Signal<bool>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
    /// HTML `autocomplete`, e.g. `"off"` for keys: the browser then remembers nothing.
    #[prop(optional)]
    autocomplete: Option<&'static str>,
) -> impl IntoView {
    let id = next_id("field");
    view! {
        <div class="ui-field" data-state=move || error.get().then_some("error")>
            <label class="ui-field__label" for=id.clone()>{label}</label>
            <input
                class="ui-field__input"
                id=id
                type=input_type
                autocomplete=autocomplete
                spellcheck=autocomplete.map(|_| "false")
                placeholder=placeholder
                disabled=move || disabled.get()
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
            {hint.map(|h| view! { <span class="ui-field__hint">{h}</span> })}
        </div>
    }
}

/// Search field with magnifier. The placeholder is a text or a signal, so it can follow the
/// language; it is also the label for screen readers.
#[component]
pub fn SearchField(
    value: RwSignal<String>,
    #[prop(into, default = Signal::stored("Suchen".to_string()))] placeholder: Signal<String>,
) -> impl IntoView {
    let id = next_id("search");
    view! {
        <div class="ui-search">
            <Icon path=icons::SEARCH size=17 stroke=2.2 />
            <label class="ui-sr-only" for=id.clone()>{move || placeholder.get()}</label>
            <input
                class="ui-field__input"
                id=id
                type="search"
                placeholder=move || placeholder.get()
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
        </div>
    }
}
