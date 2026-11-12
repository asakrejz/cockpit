use leptos::prelude::*;

use crate::{Icon, icons};

/// Schalter (`Toggle`). Gewöhnlich in einer `ListRow` mit `label=true`.
#[component]
pub fn Toggle(
    checked: RwSignal<bool>,
    /// Zugänglicher Name, wenn kein sichtbares Label in der Nähe steht.
    #[prop(optional, into)]
    aria_label: Option<String>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
) -> impl IntoView {
    view! {
        <span class="ui-toggle">
            <input
                type="checkbox"
                role="switch"
                aria-label=aria_label
                prop:checked=move || checked.get()
                disabled=move || disabled.get()
                on:change=move |ev| checked.set(event_target_checked(&ev))
            />
        </span>
    }
}

/// Segmentierter Picker (`Picker` mit `.segmented`).
///
/// `options` sind Paare aus Wert und Beschriftung.
#[component]
pub fn Segmented(
    options: &'static [(&'static str, &'static str)],
    value: RwSignal<&'static str>,
    #[prop(into)] aria_label: String,
) -> impl IntoView {
    view! {
        <div class="ui-segmented" role="group" aria-label=aria_label>
            {options
                .iter()
                .map(|&(id, label)| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (value.get() == id).to_string()
                            on:click=move |_| value.set(id)
                        >
                            {label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// Segmentierter Picker, dessen Wert von außen kommt (z. B. aus einem Datenmodell).
///
/// Anders als [`Segmented`] schreibt er nicht selbst, sondern meldet die Auswahl über
/// `on_change`. So bleibt das Modell die einzige Quelle der Wahrheit.
#[component]
pub fn SegmentedPicker(
    options: &'static [(&'static str, &'static str)],
    #[prop(into)] value: Signal<&'static str>,
    #[prop(into)] on_change: Callback<&'static str>,
    #[prop(into)] aria_label: String,
    /// Füllt die volle Breite; alle Segmente sind gleich breit.
    #[prop(optional)]
    stretch: bool,
) -> impl IntoView {
    let (group_css, button_css) = if stretch {
        (
            "align-self: stretch",
            Some("flex: 1 1 0; padding-inline: 8px"),
        )
    } else {
        ("align-self: flex-start", None)
    };
    view! {
        <div class="ui-segmented" role="group" aria-label=aria_label style=group_css>
            {options
                .iter()
                .map(|&(id, label)| {
                    view! {
                        <button
                            type="button"
                            style=button_css
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

/// Auswahlliste (`Picker` mit `.menu`): ein natives `<select>`, damit Tastatur, Screenreader
/// und Mobilgeräte sie ohne eigenes Menü bedienen.
///
/// `options` sind Paare aus Wert und Beschriftung und dürfen sich ändern (z. B. wenn sie
/// vom Server kommen). Wie [`SegmentedPicker`] schreibt sie nicht selbst, sondern meldet
/// die Auswahl über `on_change`.
#[component]
pub fn Select(
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] on_change: Callback<String>,
    /// Sichtbare Beschriftung darüber. Ohne sie braucht es `aria_label`.
    #[prop(optional, into)]
    label: Option<String>,
    #[prop(optional, into)] aria_label: Option<String>,
    /// Kleiner, für Kopfzeilen und Werkzeugleisten.
    #[prop(optional)]
    compact: bool,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
) -> impl IntoView {
    let id = format!("ui-select-{}", next_id());
    let select = view! {
        <select
            id=id.clone()
            class="ui-field__input ui-select"
            data-size=compact.then_some("small")
            aria-label=aria_label
            disabled=move || disabled.get()
            on:change=move |ev| on_change.run(event_target_value(&ev))
        >
            {move || {
                let current = value.get();
                options
                    .get()
                    .into_iter()
                    .map(|(v, l)| {
                        let selected = v == current;
                        view! { <option value=v selected=selected>{l}</option> }
                    })
                    .collect_view()
            }}
        </select>
    };
    view! {
        <span class="ui-field ui-select-field">
            {label.map(|l| view! { <label class="ui-field__label" for=id>{l}</label> })}
            {select}
        </span>
    }
}

/// Fortlaufende Nummer für eindeutige `id`s (Label ↔ Eingabe).
fn next_id() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Schieberegler (`Slider`).
#[component]
pub fn Slider(
    value: RwSignal<i32>,
    #[prop(default = 0)] min: i32,
    #[prop(default = 100)] max: i32,
    #[prop(into)] aria_label: String,
) -> impl IntoView {
    let fill = move || {
        let span = (max - min).max(1);
        format!("--p: {}%", (value.get() - min) * 100 / span)
    };
    view! {
        <input
            class="ui-slider"
            type="range"
            min=min
            max=max
            aria-label=aria_label
            prop:value=move || value.get()
            style=fill
            on:input=move |ev| {
                if let Ok(v) = event_target_value(&ev).parse() {
                    value.set(v);
                }
            }
        />
    }
}

/// Plus/Minus-Zähler (`Stepper`).
#[component]
pub fn Stepper(
    value: RwSignal<i32>,
    #[prop(default = 0)] min: i32,
    #[prop(default = 99)] max: i32,
    #[prop(default = 1)] step: i32,
) -> impl IntoView {
    let at_min = move || value.get() <= min;
    let at_max = move || value.get() >= max;
    view! {
        <div class="ui-stepper">
            <button
                type="button"
                aria-label="Weniger"
                disabled=at_min
                on:click=move |_| value.update(|v| *v = (*v - step).max(min))
            >
                <Icon path=icons::MINUS size=16 stroke=2.4 />
            </button>
            <span class="ui-stepper__divider"></span>
            <button
                type="button"
                aria-label="Mehr"
                disabled=at_max
                on:click=move |_| value.update(|v| *v = (*v + step).min(max))
            >
                <Icon path=icons::PLUS size=16 stroke=2.4 />
            </button>
        </div>
    }
}
