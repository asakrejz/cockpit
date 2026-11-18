//! Controls with texts that are chosen at runtime (the language can change). The ones in
//! `feature_ui` take `&'static str` labels, which cannot follow the language.

use feature_ui::*;
use leptos::prelude::*;

/// Like `SegmentedPicker`, but the labels are owned texts.
#[component]
pub fn Picker(
    options: Vec<(&'static str, String)>,
    #[prop(into)] value: Signal<&'static str>,
    #[prop(into)] on_change: Callback<&'static str>,
    #[prop(into)] aria_label: String,
) -> impl IntoView {
    view! {
        <div class="ui-segmented" role="group" aria-label=aria_label style="align-self: flex-start">
            {options
                .into_iter()
                .map(|(id, label)| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (value.get() == id).to_string()
                            on:click=move |_| on_change.run(id)
                        >
                            {label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// Like `SearchField`, but the placeholder is an owned text.
#[component]
pub fn SearchBox(value: RwSignal<String>, #[prop(into)] placeholder: String) -> impl IntoView {
    view! {
        <div class="ui-search">
            <Icon path=icons::SEARCH size=17 stroke=2.2 />
            <input
                class="ui-field__input"
                type="search"
                aria-label=placeholder.clone()
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
        </div>
    }
}
