use leptos::prelude::*;

use crate::{Icon, icons};

/// Switch (`Toggle`). Usually in a `ListRow` with `label=true`.
#[component]
pub fn Toggle(
    checked: RwSignal<bool>,
    /// Accessible name if no visible label is nearby.
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

/// Segmented picker (`Picker` with `.segmented`).
///
/// `options` are pairs of value and label.
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

/// Segmented picker whose value comes from outside (e.g. from a data model).
///
/// Unlike [`Segmented`] it does not write itself but reports the selection via
/// `on_change`. That way the model stays the only source of truth.
#[component]
pub fn SegmentedPicker(
    options: &'static [(&'static str, &'static str)],
    #[prop(into)] value: Signal<&'static str>,
    #[prop(into)] on_change: Callback<&'static str>,
    #[prop(into)] aria_label: String,
    /// Fills the full width; all segments are equally wide.
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

/// Selection list (`Picker` with `.menu`): a native `<select>` so that keyboard, screen reader
/// and mobile devices operate it without a menu of its own.
///
/// `options` are pairs of value and label and may change (e.g. when they
/// come from the server). Like [`SegmentedPicker`] it does not write itself but reports
/// the selection via `on_change`.
#[component]
pub fn Select(
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] on_change: Callback<String>,
    /// Visible label above. Without it `aria_label` is needed.
    #[prop(optional, into)]
    label: Option<String>,
    #[prop(optional, into)] aria_label: Option<String>,
    /// Smaller, for headers and toolbars.
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

/// Running number for unique `id`s (label ↔ input).
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

/// Plus/minus counter (`Stepper`).
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
                aria-label=crate::t!(UiStepperLess)
                disabled=at_min
                on:click=move |_| value.update(|v| *v = (*v - step).max(min))
            >
                <Icon path=icons::MINUS size=16 stroke=2.4 />
            </button>
            <span class="ui-stepper__divider"></span>
            <button
                type="button"
                aria-label=crate::t!(UiStepperMore)
                disabled=at_max
                on:click=move |_| value.update(|v| *v = (*v + step).min(max))
            >
                <Icon path=icons::PLUS size=16 stroke=2.4 />
            </button>
        </div>
    }
}
