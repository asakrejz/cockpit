//! Fortschritt: Timeline, Balken, Protokoll und Schritt-Anzeige.

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

/// Senkrechte Abfolge von Schritten. Kinder: [`TimelineStep`].
#[component]
pub fn Timeline(children: Children) -> impl IntoView {
    view! { <ol class="ui-timeline">{children()}</ol> }
}

/// Schritt der Timeline. Erledigte zeigen ein Häkchen, der aktive pulsiert.
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

/// Fortschrittsbalken (`value` von `max`).
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

/// Protokoll in Festbreitenschrift. Screenreader bekommen neue Zeilen angesagt.
#[component]
pub fn Log(#[prop(into)] text: Signal<String>) -> impl IntoView {
    view! { <pre class="ui-log" aria-live="polite">{move || text.get()}</pre> }
}

/// Schritt-Anzeige für mehrstufige Abläufe wie ein Onboarding.
#[component]
pub fn Steps(
    labels: &'static [&'static str],
    /// Index des aktuellen Schritts. Alle davor gelten als erledigt.
    #[prop(into)]
    current: Signal<usize>,
) -> impl IntoView {
    view! {
        <ol class="ui-steps" aria-label="Fortschritt">
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
