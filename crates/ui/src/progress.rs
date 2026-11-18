//! Progress: timeline, bar, log and step indicator.

use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepState {
    #[default]
    Pending,
    Active,
    Done,
}

impl StepState {
    pub fn id(self) -> &'static str {
        match self {
            StepState::Pending => "pending",
            StepState::Active => "active",
            StepState::Done => "done",
        }
    }
}

/// Vertical sequence of steps. Children: [`TimelineStep`].
#[component]
pub fn Timeline(children: Children) -> impl IntoView {
    view! { <ol class="ui-timeline">{children()}</ol> }
}

/// Step of the timeline. Finished ones show a checkmark, the active one pulses.
#[component]
pub fn TimelineStep(
    number: usize,
    #[prop(into)] state: Signal<StepState>,
    #[prop(into)] title: Signal<String>,
    #[prop(into)] text: Signal<String>,
) -> impl IntoView {
    view! {
        <li class="ui-timeline__step" data-state=move || state.get().id()>
            <span class="ui-timeline__icon" aria-hidden="true">
                {move || if state.get() == StepState::Done { "✓".to_string() } else { number.to_string() }}
            </span>
            <div class="ui-vstack" style="--spacing: 2px; padding-top: 5px">
                <span class="t-headline">{move || title.get()}</span>
                <span class="t-subheadline t-secondary">{move || text.get()}</span>
            </div>
        </li>
    }
}

/// Progress bar (`value` of `max`).
#[component]
pub fn Progress(
    #[prop(into)] value: Signal<usize>,
    max: usize,
    #[prop(into)] label: String,
) -> impl IntoView {
    let width = move || format!("width: {}%", value.get().min(max) * 100 / max.max(1));
    view! {
        <div
            class="ui-progress"
            role="progressbar"
            aria-label=label
            aria-valuemin="0"
            aria-valuemax=max
            aria-valuenow=move || value.get()
        >
            <div class="ui-progress__bar" style=width></div>
        </div>
    }
}

/// Log in monospace. Screen readers get new lines announced.
#[component]
pub fn Log(#[prop(into)] text: Signal<String>) -> impl IntoView {
    view! { <pre class="ui-log" aria-live="polite">{move || text.get()}</pre> }
}

/// Step indicator for multi-step flows such as an onboarding.
#[component]
pub fn Steps(
    labels: &'static [&'static str],
    /// Index of the current step. All before it count as done.
    #[prop(into)]
    current: Signal<usize>,
) -> impl IntoView {
    view! {
        <ol class="ui-steps" aria-label=crate::t!(UiProgress)>
            {labels
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    view! {
                        <li
                            class="ui-steps__item"
                            aria-current=move || (current.get() == i).then_some("step")
                            data-done=move || (current.get() > i).then_some("")
                        >
                            <span class="ui-steps__no">{i + 1}</span>
                            {*label}
                        </li>
                    }
                })
                .collect_view()}
        </ol>
    }
}
