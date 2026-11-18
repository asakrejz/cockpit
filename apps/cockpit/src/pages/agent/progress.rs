//! Usage and the course in five steps.

use crate::route::Route;
use cockpit_core::{agent::STEPS, usage::format_euro};
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
        <div class="ui-list__section" role="region" aria-label=t!(AgentProgressUsage)>
            <p class="ui-list__header">{t!(AgentProgressUsage)}</p>
            <div class="ck-usage">
                <p class="t-subheadline">
                    {match budget {
                        Some(b) => t!(
                            AgentProgressThisRunBudget,
                            usage = super::labels::usage_line(&usage),
                            budget = format_euro(b)
                        ),
                        None => t!(AgentProgressThisRun, usage = super::labels::usage_line(&usage)),
                    }}
                </p>
                <p class="t-footnote t-secondary">
                    {match limits.month_budget_cents {
                        Some(b) => t!(
                            AgentProgressTotalsBudget,
                            today = format_euro(today.cost_cents),
                            month = format_euro(month.cost_cents),
                            budget = format_euro(b)
                        ),
                        None => t!(
                            AgentProgressTotals,
                            today = format_euro(today.cost_cents),
                            month = format_euro(month.cost_cents)
                        ),
                    }}
                    " · "
                    <a class="ui-link" href=Route::Limits.hash()>{t!(AgentProgressLimitsLink)}</a>
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
                            Some(u) => format!("{} · {}", s.text, super::labels::usage_line(&u)),
                            None => s.text,
                        })
                        .unwrap_or_default()
                })
            />
        }
    })
    .collect_view()
}
