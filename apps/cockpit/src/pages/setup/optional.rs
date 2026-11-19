//! A collapsed section of the setup: closed until someone opens it. What is inside is optional.

use leptos::prelude::*;

/// A native `<details>`. `opened` turns true the first time it is opened (the content can load
/// then, not before) and stays true.
#[component]
pub fn Optional(title: String, opened: RwSignal<bool>, children: Children) -> impl IntoView {
    view! {
        <details
            class="ck-optional"
            on:toggle=move |ev| {
                let open = ev
                    .target()
                    .and_then(|t| js_sys::Reflect::get(&t, &"open".into()).ok())
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if open && !opened.get_untracked() {
                    opened.set(true);
                }
            }
        >
            <summary class="t-headline">{title}</summary>
            <div class="ck-optional__body">{children()}</div>
        </details>
    }
}
