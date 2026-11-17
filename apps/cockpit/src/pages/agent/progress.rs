//! Usage and the course in five steps.

use crate::route::Route;
use cockpit_core::{
    agent::STEPS,
    usage::{format_euro, format_number},
};
use feature_ui::*;
use leptos::prelude::*;

use super::context::AgentCtx;

/// Usage of this run and totals over all runs.
pub(super) fn usage(ctx: AgentCtx) -> Option<impl IntoView> {
    let AgentCtx { store, run, .. } = ctx;
    // Usage of this run and totals over all runs.
    let run = run.get().filter(|r| !r.is_idle())?;
    let usage = run.usage();
    let (limits, today, month) = store.ws.with(|w| {
        let now = store.now.get();
        (w.limits.clone(), w.spent_today(now), w.spent_month(now))
    });
    let budget = limits.run_budget_cents.map(|b| b + run.extra_budget_cents);
    Some(view! {
        <div class="ui-list__section" role="region" aria-label="Verbrauch">
            <p class="ui-list__header">"Verbrauch"</p>
            <div class="ck-usage">
                <p class="t-subheadline">
                    {format!(
                        "Dieser Lauf: {} Token ein · {} aus · {} · {}",
                        format_number(usage.input_tokens),
                        format_number(usage.output_tokens),
                        format_euro(usage.cost_cents),
                        cockpit_core::usage::format_duration(usage.millis),
                    )}
                    {budget.map(|b| format!(" (Budget {})", format_euro(b)))}
                </p>
                <p class="t-footnote t-secondary">
                    {format!(
                        "Heute {} · diesen Monat {}{}",
                        format_euro(today.cost_cents),
                        format_euro(month.cost_cents),
                        limits
                            .month_budget_cents
                            .map(|b| format!(" von {}", format_euro(b)))
                            .unwrap_or_default(),
                    )}
                    " · "
                    <a class="ui-link" href=Route::Limits.hash()>"Grenzen & Regeln"</a>
                </p>
            </div>
        </div>
    })
}

/// The state of a step as the timeline shows it.
fn step_state(state: cockpit_core::StepState) -> StepState {
    match state {
        cockpit_core::StepState::Pending => StepState::Pending,
        cockpit_core::StepState::Active => StepState::Active,
        cockpit_core::StepState::Done => StepState::Done,
    }
}

/// The five steps of the run.
pub(super) fn timeline_steps(ctx: AgentCtx) -> impl IntoView {
    let AgentCtx { steps, .. } = ctx;
    (0..STEPS)
    .map(|i| {
        let step = move || steps.get().get(i).cloned();
        view! {
            <TimelineStep
                number=i + 1
                state=Signal::derive(move || step().map(|s| step_state(s.state)).unwrap_or_default())
                title=Signal::derive(move || step().map(|s| s.title).unwrap_or_default())
                text=Signal::derive(move || {
                    step()
                        .map(|s| match s.usage {
                            Some(u) => format!("{} · {}", s.text, u.line()),
                            None => s.text,
                        })
                        .unwrap_or_default()
                })
            />
        }
    })
    .collect_view()
}
