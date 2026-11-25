//! Controls with texts that are chosen at runtime (the language can change). The ones in
//! `feature_ui` take `&'static str` labels, which cannot follow the language.

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
        <div class="ui-segmented ui-self-start" role="group" aria-label=aria_label>
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
