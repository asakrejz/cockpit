//! Ein einfacher Zähler – das "Hello World" der reaktiven UIs.
//!
//! Zeigt: Komponenten, Props mit Default-Werten und Signals.

use leptos::prelude::*;

/// Ein Zähler mit Plus-, Minus- und Reset-Button.
///
/// ```ignore
/// view! { <Counter initial=10 step=5 /> }
/// ```
#[component]
pub fn Counter(
    /// Startwert (und Wert nach "Reset").
    #[prop(default = 0)]
    initial: i32,
    /// Um wie viel pro Klick erhöht/verringert wird.
    #[prop(default = 1)]
    step: i32,
) -> impl IntoView {
    // `signal` liefert ein Lese- und ein Schreib-Handle.
    // Alles, was `count` liest, wird automatisch neu gerendert.
    let (count, set_count) = signal(initial);

    view! {
        <div class="counter">
            <button on:click=move |_| set_count.update(|c| *c -= step)>"−"</button>
            <span class="counter__value">{count}</span>
            <button on:click=move |_| set_count.update(|c| *c += step)>"+"</button>
            <button class="counter__reset" on:click=move |_| set_count.set(initial)>
                "Reset"
            </button>
        </div>
    }
}
